/// Model-level routing table for LiteLLM-style model_list configs.
///
/// Maps virtual model names to one or more backend deployments.
/// Uses lock-free atomics for round-robin counters and approximate
/// RPM/TPM tracking (60-second tumbling windows).
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
    // Approximate 60s tumbling window counters.
    rpm_used: AtomicU32,
    tpm_used: AtomicU64,
    window_start_ms: AtomicU64,
}

impl Deployment {
    pub fn new(
        backend_name: String,
        actual_model: String,
        rpm_limit: Option<u32>,
        tpm_limit: Option<u64>,
    ) -> Self {
        Self {
            backend_name,
            actual_model,
            rpm_limit,
            tpm_limit,
            rpm_used: AtomicU32::new(0),
            tpm_used: AtomicU64::new(0),
            window_start_ms: AtomicU64::new(now_ms()),
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
}

/// Result of a routing decision.
pub struct RoutedDeployment<'a> {
    pub backend_name: &'a str,
    pub actual_model: &'a str,
    pub deployment: &'a Arc<Deployment>,
}

/// Maps virtual model names to backend deployments with round-robin + RPM-aware routing.
pub struct ModelRouter {
    /// model_name -> list of deployments (order = config order).
    routes: HashMap<String, Vec<Arc<Deployment>>>,
    /// Round-robin counters per model name.
    counters: HashMap<String, AtomicUsize>,
}

impl ModelRouter {
    pub fn new(routes: HashMap<String, Vec<Arc<Deployment>>>) -> Self {
        let counters = routes
            .keys()
            .map(|k| (k.clone(), AtomicUsize::new(0)))
            .collect();
        Self { routes, counters }
    }

    /// Pick the next available deployment for a model name.
    ///
    /// Round-robin with RPM-aware skip: starts at the next index in rotation,
    /// scans all deployments, skips any at their RPM limit.
    /// Returns None if the model is unknown or all deployments are at limit.
    pub fn route(&self, model_name: &str) -> Option<RoutedDeployment<'_>> {
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
        None // all at limit
    }

    /// Check if a model name exists in the routing table.
    pub fn has_model(&self, model_name: &str) -> bool {
        self.routes.contains_key(model_name)
    }

    /// Return all known model names (for /v1/models enrichment).
    pub fn known_models(&self) -> Vec<&str> {
        self.routes.keys().map(|s| s.as_str()).collect()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
}
