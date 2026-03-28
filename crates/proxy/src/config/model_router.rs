/// Model-level routing table for LiteLLM-style model_list configs.
///
/// Maps virtual model names to one or more backend deployments.
/// Uses lock-free atomics for round-robin counters and approximate
/// RPM/TPM tracking (60-second tumbling windows).
///
/// Supports multiple routing strategies: round-robin (default),
/// least-busy (lowest in-flight), latency-based (lowest EWMA),
/// weighted round-robin, and cost-based (lowest price-per-token).
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Routing strategy for selecting among multiple deployments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingStrategy {
    /// Round-robin with RPM-aware skip (default, existing behavior).
    #[default]
    RoundRobin,
    /// Pick deployment with lowest in-flight request count.
    LeastBusy,
    /// Pick deployment with lowest latency EWMA.
    LatencyBased,
    /// Weighted round-robin using per-deployment weight field.
    Weighted,
    /// Pick deployment with lowest cost per token from the bundled model pricing table.
    /// Falls back to round-robin if none of the deployments have known pricing.
    CostBased,
}

/// A single backend deployment that can serve a model name.
pub struct Deployment {
    /// Key into MultiConfig.backends.
    pub backend_name: String,
    /// Model name to send to the backend (the actual provider model).
    pub actual_model: String,
    /// Per-deployment requests-per-minute limit (from LiteLLM config).
    pub rpm_limit: Option<u32>,
    /// Per-deployment tokens-per-minute limit (from LiteLLM config).
    pub tpm_limit: Option<u64>,
    /// Static weight for weighted routing (default 1).
    pub weight: u32,
    // Approximate 60s tumbling window counters.
    rpm_used: AtomicU32,
    tpm_used: AtomicU64,
    window_start_ms: AtomicU64,
    // Tracking for least-busy and latency-based routing.
    in_flight: AtomicU32,
    /// Exponentially-weighted moving average of response latency in ms.
    latency_ewma_ms: AtomicU64,
}

impl Deployment {
    pub fn new(
        backend_name: String,
        actual_model: String,
        rpm_limit: Option<u32>,
        tpm_limit: Option<u64>,
    ) -> Self {
        Self::with_weight(backend_name, actual_model, rpm_limit, tpm_limit, 1)
    }

    pub fn with_weight(
        backend_name: String,
        actual_model: String,
        rpm_limit: Option<u32>,
        tpm_limit: Option<u64>,
        weight: u32,
    ) -> Self {
        Self {
            backend_name,
            actual_model,
            rpm_limit,
            tpm_limit,
            weight: weight.max(1), // floor at 1
            rpm_used: AtomicU32::new(0),
            tpm_used: AtomicU64::new(0),
            window_start_ms: AtomicU64::new(now_ms()),
            in_flight: AtomicU32::new(0),
            latency_ewma_ms: AtomicU64::new(0),
        }
    }

    /// Check and reset the window if >60s have elapsed. Returns true if reset occurred.
    fn maybe_reset_window(&self) -> bool {
        let now = now_ms();
        let start = self.window_start_ms.load(Ordering::Relaxed);
        if now.saturating_sub(start) > 60_000 {
            // CAS to avoid double-reset from concurrent callers.
            if self
                .window_start_ms
                .compare_exchange(start, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                self.rpm_used.store(0, Ordering::Relaxed);
                self.tpm_used.store(0, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Returns true if this deployment is under its RPM limit (or has no limit).
    fn under_rpm_limit(&self) -> bool {
        self.maybe_reset_window();
        match self.rpm_limit {
            Some(limit) => self.rpm_used.load(Ordering::Relaxed) < limit,
            None => true,
        }
    }

    /// Increment RPM counter. Called when a request is routed here.
    fn record_request(&self) {
        self.rpm_used.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment TPM counter. Called after response with actual token count.
    pub fn record_tokens(&self, tokens: u64) {
        self.tpm_used.fetch_add(tokens, Ordering::Relaxed);
    }

    /// Mark a request as dispatched. Call before sending to backend.
    pub fn record_start(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    /// Mark a request as completed. Updates in-flight count and latency EWMA.
    /// Call after response (or error) with wall-clock elapsed ms.
    pub fn record_finish(&self, latency_ms: u64) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        // EWMA with alpha=0.3: new = 0.3 * sample + 0.7 * old.
        // CAS loop for lock-free update. Approximate is fine.
        loop {
            let old = self.latency_ewma_ms.load(Ordering::Relaxed);
            let new_val = if old == 0 {
                latency_ms
            } else {
                (3 * latency_ms + 7 * old) / 10
            };
            if self
                .latency_ewma_ms
                .compare_exchange(old, new_val, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Current in-flight request count.
    pub fn in_flight_count(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Current latency EWMA in ms.
    pub fn latency_ms(&self) -> u64 {
        self.latency_ewma_ms.load(Ordering::Relaxed)
    }
}

/// Result of a routing decision.
pub struct RoutedDeployment<'a> {
    pub backend_name: &'a str,
    pub actual_model: &'a str,
    pub deployment: &'a Arc<Deployment>,
}

/// Maps virtual model names to backend deployments with configurable routing.
pub struct ModelRouter {
    /// model_name -> list of deployments (order = config order).
    routes: HashMap<String, Vec<Arc<Deployment>>>,
    /// Round-robin counters per model name (used by RoundRobin and Weighted).
    counters: HashMap<String, AtomicUsize>,
    /// Routing strategy applied to all models.
    strategy: RoutingStrategy,
}

impl ModelRouter {
    pub fn new(routes: HashMap<String, Vec<Arc<Deployment>>>) -> Self {
        Self::with_strategy(routes, RoutingStrategy::default())
    }

    pub fn with_strategy(
        routes: HashMap<String, Vec<Arc<Deployment>>>,
        strategy: RoutingStrategy,
    ) -> Self {
        let counters = routes
            .keys()
            .map(|k| (k.clone(), AtomicUsize::new(0)))
            .collect();
        Self {
            routes,
            counters,
            strategy,
        }
    }

    /// Pick the next available deployment for a model name.
    ///
    /// Dispatches to the configured routing strategy. All strategies
    /// skip deployments that are at their RPM limit.
    /// Returns None if the model is unknown or all deployments are at limit.
    pub fn route(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
        match self.strategy {
            RoutingStrategy::RoundRobin => self.route_round_robin(model_name),
            RoutingStrategy::LeastBusy => self.route_least_busy(model_name),
            RoutingStrategy::LatencyBased => self.route_latency_based(model_name),
            RoutingStrategy::Weighted => self.route_weighted(model_name),
            RoutingStrategy::CostBased => self.route_cost_based(model_name),
        }
    }

    /// Round-robin with RPM-aware skip.
    fn route_round_robin(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
        let deployments = self.routes.get(model_name)?;
        let counter = self.counters.get(model_name)?;
        let len = deployments.len();
        if len == 0 {
            return None;
        }

        let start = counter.fetch_add(1, Ordering::Relaxed) % len;
        for i in 0..len {
            let idx = (start + i) % len;
            let d = &deployments[idx];
            if d.under_rpm_limit() {
                d.record_request();
                return Some(RoutedDeployment {
                    backend_name: &d.backend_name,
                    actual_model: &d.actual_model,
                    deployment: d,
                });
            }
        }
        None
    }

    /// Pick deployment with lowest in-flight count (ties broken by config order).
    fn route_least_busy(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
        let deployments = self.routes.get(model_name)?;
        if deployments.is_empty() {
            return None;
        }

        let mut best: Option<(usize, u32)> = None;
        for (i, d) in deployments.iter().enumerate() {
            if !d.under_rpm_limit() {
                continue;
            }
            let count = d.in_flight_count();
            if best.is_none() || count < best.unwrap().1 {
                best = Some((i, count));
            }
        }

        best.map(|(idx, _)| {
            let d = &deployments[idx];
            d.record_request();
            RoutedDeployment {
                backend_name: &d.backend_name,
                actual_model: &d.actual_model,
                deployment: d,
            }
        })
    }

    /// Pick deployment with lowest latency EWMA. Zero (no data yet) is naturally
    /// the minimum, so unknown deployments get tried first for warmup.
    fn route_latency_based(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
        let deployments = self.routes.get(model_name)?;
        if deployments.is_empty() {
            return None;
        }

        let mut best: Option<(usize, u64)> = None;
        for (i, d) in deployments.iter().enumerate() {
            if !d.under_rpm_limit() {
                continue;
            }
            let lat = d.latency_ms();
            if best.is_none() || lat < best.unwrap().1 {
                best = Some((i, lat));
            }
        }

        best.map(|(idx, _)| {
            let d = &deployments[idx];
            d.record_request();
            RoutedDeployment {
                backend_name: &d.backend_name,
                actual_model: &d.actual_model,
                deployment: d,
            }
        })
    }

    /// Weighted round-robin. Deployments with weight=3 get 3x traffic vs weight=1.
    /// Uses a virtual counter that expands by total weight per cycle.
    fn route_weighted(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
        let deployments = self.routes.get(model_name)?;
        let counter = self.counters.get(model_name)?;
        let len = deployments.len();
        if len == 0 {
            return None;
        }

        // Build expanded index: deployment i appears weight[i] times.
        let total_weight: usize = deployments.iter().map(|d| d.weight as usize).sum();
        if total_weight == 0 {
            return None;
        }

        let tick = counter.fetch_add(1, Ordering::Relaxed) % total_weight;
        let mut cumulative = 0usize;

        // Find which deployment this tick maps to.
        let mut start_idx = 0;
        for (i, d) in deployments.iter().enumerate() {
            cumulative += d.weight as usize;
            if tick < cumulative {
                start_idx = i;
                break;
            }
        }

        // Try starting at the weighted pick, then scan others if RPM-limited.
        for i in 0..len {
            let idx = (start_idx + i) % len;
            let d = &deployments[idx];
            if d.under_rpm_limit() {
                d.record_request();
                return Some(RoutedDeployment {
                    backend_name: &d.backend_name,
                    actual_model: &d.actual_model,
                    deployment: d,
                });
            }
        }
        None
    }

    /// Pick deployment with the lowest combined cost per token (input + output).
    ///
    /// Uses the global model pricing table. Deployments with unknown pricing are
    /// treated as having infinite cost and are skipped in favour of priced ones.
    /// If no deployment has known pricing, falls back to round-robin.
    fn route_cost_based(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
        let deployments = self.routes.get(model_name)?;
        if deployments.is_empty() {
            return None;
        }

        let pricing = crate::cost::pricing();
        let mut best: Option<(usize, f64)> = None;
        let mut any_priced = false;

        for (i, d) in deployments.iter().enumerate() {
            if !d.under_rpm_limit() {
                continue;
            }
            if let Some((input, output)) = pricing.price_for_model(&d.actual_model) {
                any_priced = true;
                let score = input + output;
                if best.is_none() || score < best.unwrap().1 {
                    best = Some((i, score));
                }
            }
        }

        // No deployment has known pricing; fall back to round-robin.
        if !any_priced {
            return self.route_round_robin(model_name);
        }

        best.map(|(idx, _)| {
            let d = &deployments[idx];
            d.record_request();
            RoutedDeployment {
                backend_name: &d.backend_name,
                actual_model: &d.actual_model,
                deployment: d,
            }
        })
    }

    /// Check if a model name exists in the routing table.
    pub fn has_model(&self, model_name: &str) -> bool {
        self.routes.contains_key(model_name)
    }

    /// Return all known model names (for /v1/models enrichment).
    pub fn known_models(&self) -> Vec<&str> {
        self.routes.keys().map(|s| s.as_str()).collect()
    }

    /// Current routing strategy.
    pub fn strategy(&self) -> RoutingStrategy {
        self.strategy
    }

    /// Add a deployment for a model name (for dynamic model management).
    pub fn add_deployment(&mut self, model_name: String, deployment: Arc<Deployment>) {
        let deps = self.routes.entry(model_name.clone()).or_default();
        deps.push(deployment);
        self.counters
            .entry(model_name)
            .or_insert_with(|| AtomicUsize::new(0));
    }

    /// Remove all deployments for a model name. Returns true if the model existed.
    pub fn remove_model(&mut self, model_name: &str) -> bool {
        let removed = self.routes.remove(model_name).is_some();
        self.counters.remove(model_name);
        removed
    }

    /// List all models with their deployment counts (for admin API).
    pub fn list_models(&self) -> Vec<(&str, usize)> {
        self.routes
            .iter()
            .map(|(name, deps)| (name.as_str(), deps.len()))
            .collect()
    }
}

fn now_ms() -> u64 {
    crate::admin::keys::now_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_deployments(specs: &[(&str, &str, Option<u32>)]) -> Vec<Arc<Deployment>> {
        specs
            .iter()
            .map(|(backend, model, rpm)| {
                Arc::new(Deployment::new(
                    backend.to_string(),
                    model.to_string(),
                    *rpm,
                    None,
                ))
            })
            .collect()
    }

    fn make_weighted(specs: &[(&str, &str, u32)]) -> Vec<Arc<Deployment>> {
        specs
            .iter()
            .map(|(backend, model, weight)| {
                Arc::new(Deployment::with_weight(
                    backend.to_string(),
                    model.to_string(),
                    None,
                    None,
                    *weight,
                ))
            })
            .collect()
    }

    #[test]
    fn round_robin_across_deployments() {
        let deps = make_deployments(&[
            ("azure_0", "gpt-4o", None),
            ("openai_0", "gpt-4o", None),
            ("azure_1", "gpt-4o", None),
        ]);
        let mut routes = HashMap::new();
        routes.insert("gpt-4o".to_string(), deps);
        let router = ModelRouter::new(routes);

        let r0 = router.route("gpt-4o").unwrap();
        let r1 = router.route("gpt-4o").unwrap();
        let r2 = router.route("gpt-4o").unwrap();
        let r3 = router.route("gpt-4o").unwrap();

        // Should cycle through all three backends
        assert_eq!(r0.backend_name, "azure_0");
        assert_eq!(r1.backend_name, "openai_0");
        assert_eq!(r2.backend_name, "azure_1");
        assert_eq!(r3.backend_name, "azure_0"); // wraps around
    }

    #[test]
    fn rpm_aware_skip() {
        let deps = make_deployments(&[
            ("backend_a", "model-x", Some(2)),
            ("backend_b", "model-x", None), // unlimited
        ]);
        let mut routes = HashMap::new();
        routes.insert("model-x".to_string(), deps);
        let router = ModelRouter::new(routes);

        // Round-robin: 0->a, 1->b, 2->a, 3->b (all under limit so far)
        let r0 = router.route("model-x").unwrap();
        assert_eq!(r0.backend_name, "backend_a");
        let r1 = router.route("model-x").unwrap();
        assert_eq!(r1.backend_name, "backend_b");
        let r2 = router.route("model-x").unwrap();
        assert_eq!(r2.backend_name, "backend_a"); // backend_a now at limit (2 requests)
        let r3 = router.route("model-x").unwrap();
        assert_eq!(r3.backend_name, "backend_b"); // normal round-robin

        // Request 4 would go to backend_a (index 0) but it's at limit, skip to backend_b
        let r4 = router.route("model-x").unwrap();
        assert_eq!(r4.backend_name, "backend_b");
    }

    #[test]
    fn all_at_limit_returns_none() {
        let deps = make_deployments(&[("only", "m", Some(1))]);
        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::new(routes);

        assert!(router.route("m").is_some()); // first request ok
        assert!(router.route("m").is_none()); // at limit
    }

    #[test]
    fn unknown_model_returns_none() {
        let router = ModelRouter::new(HashMap::new());
        assert!(router.route("nonexistent").is_none());
    }

    #[test]
    fn has_model_check() {
        let deps = make_deployments(&[("b", "m", None)]);
        let mut routes = HashMap::new();
        routes.insert("gpt-4o".to_string(), deps);
        let router = ModelRouter::new(routes);

        assert!(router.has_model("gpt-4o"));
        assert!(!router.has_model("gpt-3.5"));
    }

    #[test]
    fn single_deployment() {
        let deps = make_deployments(&[("sole", "the-model", None)]);
        let mut routes = HashMap::new();
        routes.insert("alias".to_string(), deps);
        let router = ModelRouter::new(routes);

        for _ in 0..10 {
            let r = router.route("alias").unwrap();
            assert_eq!(r.backend_name, "sole");
            assert_eq!(r.actual_model, "the-model");
        }
    }

    #[test]
    fn known_models_returns_all() {
        let mut routes = HashMap::new();
        routes.insert("gpt-4o".to_string(), make_deployments(&[("b", "m", None)]));
        routes.insert(
            "claude-3".to_string(),
            make_deployments(&[("b", "m", None)]),
        );
        let router = ModelRouter::new(routes);

        let mut models = router.known_models();
        models.sort();
        assert_eq!(models, vec!["claude-3", "gpt-4o"]);
    }

    // ---- Least-busy strategy tests ----

    #[test]
    fn least_busy_picks_lowest_in_flight() {
        let deps = make_deployments(&[("a", "m", None), ("b", "m", None), ("c", "m", None)]);
        // Simulate: a has 5 in-flight, b has 1, c has 3.
        deps[0].in_flight.store(5, Ordering::Relaxed);
        deps[1].in_flight.store(1, Ordering::Relaxed);
        deps[2].in_flight.store(3, Ordering::Relaxed);

        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::LeastBusy);

        let r = router.route("m").unwrap();
        assert_eq!(r.backend_name, "b");
    }

    #[test]
    fn least_busy_skips_rpm_limited() {
        let deps = make_deployments(&[("a", "m", Some(1)), ("b", "m", None)]);
        deps[1].in_flight.store(100, Ordering::Relaxed);

        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::LeastBusy);

        // First request goes to a (lowest in-flight=0)
        let r0 = router.route("m").unwrap();
        assert_eq!(r0.backend_name, "a");
        // a is now at RPM limit (1), next goes to b despite high in-flight
        let r1 = router.route("m").unwrap();
        assert_eq!(r1.backend_name, "b");
    }

    // ---- Latency-based strategy tests ----

    #[test]
    fn latency_based_picks_lowest_latency() {
        let deps = make_deployments(&[
            ("fast", "m", None),
            ("slow", "m", None),
            ("medium", "m", None),
        ]);
        deps[0].latency_ewma_ms.store(50, Ordering::Relaxed);
        deps[1].latency_ewma_ms.store(500, Ordering::Relaxed);
        deps[2].latency_ewma_ms.store(200, Ordering::Relaxed);

        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::LatencyBased);

        let r = router.route("m").unwrap();
        assert_eq!(r.backend_name, "fast");
    }

    #[test]
    fn latency_based_prefers_unknown_for_warmup() {
        let deps = make_deployments(&[("known", "m", None), ("unknown", "m", None)]);
        deps[0].latency_ewma_ms.store(100, Ordering::Relaxed);
        // deps[1] stays at 0 (unknown)

        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::LatencyBased);

        let r = router.route("m").unwrap();
        assert_eq!(r.backend_name, "unknown"); // prefer unknown to warm it up
    }

    // ---- Weighted strategy tests ----

    #[test]
    fn weighted_distributes_by_weight() {
        let deps = make_weighted(&[("heavy", "m", 3), ("light", "m", 1)]);
        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::Weighted);

        // Over 4 requests (total weight=4): heavy gets 3, light gets 1.
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for _ in 0..4 {
            let r = router.route("m").unwrap();
            *counts.entry(r.backend_name).or_default() += 1;
        }
        assert_eq!(counts["heavy"], 3);
        assert_eq!(counts["light"], 1);
    }

    #[test]
    fn weighted_falls_back_when_rpm_limited() {
        let deps = vec![
            Arc::new(Deployment::with_weight(
                "heavy".to_string(),
                "m".to_string(),
                Some(1), // rpm limit of 1
                None,
                3,
            )),
            Arc::new(Deployment::with_weight(
                "light".to_string(),
                "m".to_string(),
                None,
                None,
                1,
            )),
        ];
        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::Weighted);

        // First request hits heavy
        let r0 = router.route("m").unwrap();
        assert_eq!(r0.backend_name, "heavy");
        // Heavy is now at RPM limit; remaining 3 ticks all fall to light
        let r1 = router.route("m").unwrap();
        assert_eq!(r1.backend_name, "light");
        let r2 = router.route("m").unwrap();
        assert_eq!(r2.backend_name, "light");
    }

    // ---- record_start / record_finish tests ----

    #[test]
    fn in_flight_tracking() {
        let d = Deployment::new("b".into(), "m".into(), None, None);
        assert_eq!(d.in_flight_count(), 0);

        d.record_start();
        d.record_start();
        assert_eq!(d.in_flight_count(), 2);

        d.record_finish(100);
        assert_eq!(d.in_flight_count(), 1);

        d.record_finish(200);
        assert_eq!(d.in_flight_count(), 0);
    }

    #[test]
    fn latency_ewma_converges() {
        let d = Deployment::new("b".into(), "m".into(), None, None);
        assert_eq!(d.latency_ms(), 0);

        // First sample sets the EWMA directly.
        d.record_finish(100);
        assert_eq!(d.latency_ms(), 100);

        // Second sample: 0.3 * 200 + 0.7 * 100 = 60 + 70 = 130.
        d.record_start(); // increment to avoid underflow
        d.record_finish(200);
        assert_eq!(d.latency_ms(), 130);
    }

    // ---- Cost-based strategy tests ----

    #[test]
    fn cost_based_picks_cheapest_model() {
        // gpt-4o-mini is cheaper than gpt-4o (both are in the bundled pricing table).
        let deps = make_deployments(&[("expensive", "gpt-4o", None), ("cheap", "gpt-4o-mini", None)]);
        let mut routes = HashMap::new();
        routes.insert("my-model".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::CostBased);

        // Should always pick the cheaper deployment.
        for _ in 0..5 {
            let r = router.route("my-model").unwrap();
            assert_eq!(r.backend_name, "cheap");
        }
    }

    #[test]
    fn cost_based_skips_rpm_limited() {
        let deps = make_deployments(&[
            ("cheap-limited", "gpt-4o-mini", Some(1)),
            ("expensive-open", "gpt-4o", None),
        ]);
        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::CostBased);

        // First request: cheap-limited is available and cheapest.
        let r0 = router.route("m").unwrap();
        assert_eq!(r0.backend_name, "cheap-limited");
        // cheap-limited now at RPM limit; must use expensive-open.
        let r1 = router.route("m").unwrap();
        assert_eq!(r1.backend_name, "expensive-open");
    }

    #[test]
    fn cost_based_falls_back_to_round_robin_for_unknown_models() {
        // Unknown model names have no pricing entry; should fall back to round-robin.
        let deps = make_deployments(&[("a", "no-such-model-xyz", None), ("b", "no-such-model-xyz", None)]);
        let mut routes = HashMap::new();
        routes.insert("m".to_string(), deps);
        let router = ModelRouter::with_strategy(routes, RoutingStrategy::CostBased);

        // Should not panic and should return some deployment.
        let r0 = router.route("m").unwrap();
        let r1 = router.route("m").unwrap();
        // Round-robin order: a, b.
        assert_eq!(r0.backend_name, "a");
        assert_eq!(r1.backend_name, "b");
    }

    // ---- Mutation method tests ----

    #[test]
    fn add_deployment_to_existing_model() {
        let mut router = ModelRouter::new(HashMap::new());
        let d = Arc::new(Deployment::new("b1".into(), "m1".into(), None, None));
        router.add_deployment("my-model".to_string(), d);

        assert!(router.has_model("my-model"));
        let r = router.route("my-model").unwrap();
        assert_eq!(r.backend_name, "b1");
    }

    #[test]
    fn remove_model_works() {
        let deps = make_deployments(&[("b", "m", None)]);
        let mut routes = HashMap::new();
        routes.insert("x".to_string(), deps);
        let mut router = ModelRouter::new(routes);

        assert!(router.has_model("x"));
        assert!(router.remove_model("x"));
        assert!(!router.has_model("x"));
        assert!(!router.remove_model("x")); // idempotent
    }

    #[test]
    fn list_models_reports_counts() {
        let mut routes = HashMap::new();
        routes.insert(
            "a".to_string(),
            make_deployments(&[("b1", "m", None), ("b2", "m", None)]),
        );
        routes.insert("b".to_string(), make_deployments(&[("b1", "m", None)]));
        let router = ModelRouter::new(routes);

        let mut list = router.list_models();
        list.sort_by_key(|(name, _)| *name);
        assert_eq!(list, vec![("a", 2), ("b", 1)]);
    }
}
