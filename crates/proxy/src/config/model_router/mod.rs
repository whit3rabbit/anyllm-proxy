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
    /// Failover: scan deployments in priority order, pick the first one under its
    /// RPM limit. Sticky to the highest-priority available deployment (no rotation).
    /// This is the default for admin-DB routes (`routes.strategy = "failover"`).
    Failover,
}

impl RoutingStrategy {
    /// Map an admin-DB `routes.strategy` string to a strategy. Unknown values
    /// fall back to `Failover` (the `routes` table default).
    pub fn from_route_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "round-robin" => RoutingStrategy::RoundRobin,
            "least-busy" => RoutingStrategy::LeastBusy,
            "latency" | "latency-based" => RoutingStrategy::LatencyBased,
            "weighted" => RoutingStrategy::Weighted,
            "cost" | "cost-based" => RoutingStrategy::CostBased,
            _ => RoutingStrategy::Failover,
        }
    }
}

/// Select the index of the deployment to route to from a slice, applying the
/// given strategy and RPM-aware skipping. Records the request on the chosen
/// deployment (so callers must not call `record_request` again). Returns `None`
/// when the slice is empty or every candidate is at its RPM limit.
///
/// Shared by `ModelRouter::route` (keyed by model name) and `RouteRouter`
/// (per-route provider slice) so both use identical selection semantics.
pub(crate) fn select_from(
    deployments: &[Arc<Deployment>],
    counter: &AtomicUsize,
    strategy: RoutingStrategy,
) -> Option<usize> {
    let len = deployments.len();
    if len == 0 {
        return None;
    }
    let chosen = match strategy {
        RoutingStrategy::RoundRobin => {
            let start = counter.fetch_add(1, Ordering::Relaxed) % len;
            (0..len)
                .map(|i| (start + i) % len)
                .find(|&idx| deployments[idx].under_rpm_limit())
        }
        RoutingStrategy::Failover => {
            // Priority order (slice is already priority-sorted); no rotation.
            (0..len).find(|&idx| deployments[idx].under_rpm_limit())
        }
        RoutingStrategy::LeastBusy => min_by_metric(deployments, |d| d.in_flight_count() as u64),
        RoutingStrategy::LatencyBased => min_by_metric(deployments, |d| d.latency_ms()),
        RoutingStrategy::Weighted => {
            let total_weight: usize = deployments.iter().map(|d| d.weight as usize).sum();
            if total_weight == 0 {
                return None;
            }
            let tick = counter.fetch_add(1, Ordering::Relaxed) % total_weight;
            let mut cumulative = 0usize;
            let mut start_idx = 0;
            for (i, d) in deployments.iter().enumerate() {
                cumulative += d.weight as usize;
                if tick < cumulative {
                    start_idx = i;
                    break;
                }
            }
            (0..len)
                .map(|i| (start_idx + i) % len)
                .find(|&idx| deployments[idx].under_rpm_limit())
        }
        RoutingStrategy::CostBased => {
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
            if !any_priced {
                // No known pricing: fall back to round-robin (records internally).
                return select_from(deployments, counter, RoutingStrategy::RoundRobin);
            }
            best.map(|(idx, _)| idx)
        }
    };
    if let Some(idx) = chosen {
        deployments[idx].record_request();
    }
    chosen
}

/// Pick the index of the deployment with the smallest metric value among those
/// under their RPM limit; ties broken by slice order. `None` if all are limited.
fn min_by_metric(
    deployments: &[Arc<Deployment>],
    metric: impl Fn(&Deployment) -> u64,
) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (i, d) in deployments.iter().enumerate() {
        if !d.under_rpm_limit() {
            continue;
        }
        let m = metric(d);
        if best.is_none() || m < best.unwrap().1 {
            best = Some((i, m));
        }
    }
    best.map(|(idx, _)| idx)
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
    /// Create a deployment with a default weight of 1.
    pub fn new(
        backend_name: String,
        actual_model: String,
        rpm_limit: Option<u32>,
        tpm_limit: Option<u64>,
    ) -> Self {
        Self::with_weight(backend_name, actual_model, rpm_limit, tpm_limit, 1)
    }

    /// Create a deployment with an explicit weight (minimum 1, floored if 0).
    /// Higher weight means proportionally more traffic in weighted round-robin.
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
    /// Create a router with the default routing strategy (round-robin).
    pub fn new(routes: HashMap<String, Vec<Arc<Deployment>>>) -> Self {
        Self::with_strategy(routes, RoutingStrategy::default())
    }

    /// Create a router with an explicit routing strategy.
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
        let deployments = self.routes.get(model_name)?;
        let counter = self.counters.get(model_name)?;
        let idx = select_from(deployments, counter, self.strategy)?;
        let d = &deployments[idx];
        Some(RoutedDeployment {
            backend_name: &d.backend_name,
            actual_model: &d.actual_model,
            deployment: d,
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
mod tests;
