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
    let deps = make_deployments(&[
        ("expensive", "gpt-4o", None),
        ("cheap", "gpt-4o-mini", None),
    ]);
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
    let deps = make_deployments(&[
        ("a", "no-such-model-xyz", None),
        ("b", "no-such-model-xyz", None),
    ]);
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
fn failover_sticks_to_priority_zero_then_falls_over() {
    use std::sync::atomic::AtomicUsize;
    let deps = make_deployments(&[("primary", "m", Some(1)), ("secondary", "m", None)]);
    let counter = AtomicUsize::new(0);

    // Sticky to priority 0 while under limit (no rotation across calls).
    let i0 = select_from(&deps, &counter, RoutingStrategy::Failover).unwrap();
    assert_eq!(deps[i0].backend_name, "primary");
    // primary now at its RPM limit (1); failover to the next priority.
    let i1 = select_from(&deps, &counter, RoutingStrategy::Failover).unwrap();
    assert_eq!(deps[i1].backend_name, "secondary");
    // secondary has no limit; stays there.
    let i2 = select_from(&deps, &counter, RoutingStrategy::Failover).unwrap();
    assert_eq!(deps[i2].backend_name, "secondary");
}

#[test]
fn routing_strategy_from_route_str_maps_known_and_defaults_failover() {
    assert_eq!(
        RoutingStrategy::from_route_str("round-robin"),
        RoutingStrategy::RoundRobin
    );
    assert_eq!(
        RoutingStrategy::from_route_str("round_robin"),
        RoutingStrategy::RoundRobin
    );
    assert_eq!(
        RoutingStrategy::from_route_str("least-busy"),
        RoutingStrategy::LeastBusy
    );
    assert_eq!(
        RoutingStrategy::from_route_str("cost"),
        RoutingStrategy::CostBased
    );
    // Unknown / "failover" both map to Failover.
    assert_eq!(
        RoutingStrategy::from_route_str("failover"),
        RoutingStrategy::Failover
    );
    assert_eq!(
        RoutingStrategy::from_route_str("nonsense"),
        RoutingStrategy::Failover
    );
}

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
