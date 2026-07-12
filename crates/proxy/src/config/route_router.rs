//! Route-level dispatch table, compiled from the admin SQLite DB.
//!
//! A "route" (the `routes` table) is a named, ordered group of managed backends
//! with a strategy and per-route option overrides. This is the primary routing
//! key: the request `model` field selects a route, and the route picks a backend
//! via [`model_router::select_from`] (reusing the strategy algorithms).
//!
//! Precedence: [`RouteRouter`] is consulted before the LiteLLM `ModelRouter` and
//! the legacy default backend (see `AppState::resolve_model`). An empty router
//! (no routes configured) resolves to [`RouteResolution::NoRoute`], so existing
//! installs fall straight through and are unaffected.

use super::model_router::{select_from, Deployment, RoutingStrategy};
use crate::admin::db;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

/// Per-route option overrides. `None` on a field means "inherit the global
/// `RuntimeConfig` value" — resolution happens in `AppState`'s option accessors.
#[derive(Debug, Clone, Default)]
pub struct RouteOptions {
    pub guardrail_mode: Option<String>,
    pub pxpipe_compress: Option<bool>,
    pub pxpipe_models: Option<String>,
    pub redact_secrets: Option<bool>,
}

/// A backend assigned to a route, with the model globs it serves.
struct CompiledProvider {
    backend_name: String,
    /// Model name globs this provider serves. `"*"` matches any model.
    model_globs: Vec<String>,
    deployment: Arc<Deployment>,
}

/// A compiled, enabled route ready for dispatch.
struct CompiledRoute {
    #[allow(dead_code)]
    id: String,
    name: String,
    /// Explicit cross-route ordering (lower wins) when a model matches several routes.
    position: i32,
    strategy: RoutingStrategy,
    options: Arc<RouteOptions>,
    /// Priority-ordered (ascending) providers.
    providers: Vec<CompiledProvider>,
    /// Round-robin / weighted rotation counter for this route.
    counter: AtomicUsize,
}

/// A successful route resolution.
pub struct RouteResolved {
    pub backend_name: String,
    pub model: String,
    pub deployment: Arc<Deployment>,
    pub options: Arc<RouteOptions>,
}

/// Outcome of [`RouteRouter::resolve`].
pub enum RouteResolution {
    /// A backend was selected.
    Routed(RouteResolved),
    /// A route matched the model but every candidate is at its RPM limit.
    AllAtLimit,
    /// No enabled route serves this model — caller should fall through.
    NoRoute,
}

/// The compiled route table. Routes are stored name-sorted so the final
/// cross-route tiebreak (name ascending, after `position` and exact-match) is
/// deterministic.
pub struct RouteRouter {
    routes: Vec<CompiledRoute>,
}

impl RouteRouter {
    /// An empty router (no routes). Resolves everything to `NoRoute`.
    pub fn empty() -> Self {
        RouteRouter { routes: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Build the router from the admin DB: enabled routes, enabled route
    /// providers, and enabled managed backends only. Disabled entities and
    /// routes with no live providers are dropped.
    pub fn build_from_db(conn: &Connection) -> rusqlite::Result<RouteRouter> {
        let routes = db::list_routes(conn)?;
        let backends = db::list_managed_backends(conn)?;
        let backend_by_id: HashMap<&str, &db::ManagedBackendRow> =
            backends.iter().map(|b| (b.id.as_str(), b)).collect();

        // One shared Deployment (and thus one RPM/in-flight counter) per backend,
        // reused across every route/provider that references it — otherwise a backend
        // in N routes would track N independent counters and serve up to N× its RPM.
        // ponytail: counters still reset on each rebuild_route_router (any CRUD); carry
        // old Arcs forward if that reset ever matters for live rate-limit accuracy.
        let mut dep_by_backend: HashMap<&str, Arc<Deployment>> = HashMap::new();

        let mut compiled = Vec::new();
        for r in &routes {
            if !r.enabled {
                continue;
            }
            let provider_rows = db::list_route_providers(conn, &r.id)?;
            let mut providers = Vec::new();
            for pr in &provider_rows {
                if !pr.enabled {
                    continue;
                }
                let Some(b) = backend_by_id.get(pr.backend_id.as_str()) else {
                    continue;
                };
                if !b.enabled {
                    continue;
                }
                // actual_model is a placeholder: the requested model passes through
                // verbatim (managed backends apply no model rename). Share one Arc per
                // backend id so the RPM/in-flight counter is global to the backend.
                let deployment = dep_by_backend
                    .entry(b.id.as_str())
                    .or_insert_with(|| {
                        Arc::new(Deployment::new(b.name.clone(), String::new(), b.rpm, b.tpm))
                    })
                    .clone();
                providers.push(CompiledProvider {
                    backend_name: b.name.clone(),
                    model_globs: pr.models.clone(),
                    deployment,
                });
            }
            if providers.is_empty() {
                continue;
            }
            compiled.push(CompiledRoute {
                id: r.id.clone(),
                name: r.name.clone(),
                position: r.position,
                strategy: RoutingStrategy::from_route_str(&r.strategy),
                options: Arc::new(RouteOptions {
                    guardrail_mode: r.guardrail_mode.clone(),
                    pxpipe_compress: r.pxpipe_compress,
                    pxpipe_models: r.pxpipe_models.clone(),
                    redact_secrets: r.redact_secrets,
                }),
                providers,
                counter: AtomicUsize::new(0),
            });
        }
        compiled.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(RouteRouter { routes: compiled })
    }

    /// Resolve a request `model` to a backend.
    ///
    /// A route matches if any of its providers lists the model exactly or via
    /// `"*"`. Across matching routes the winner is chosen by, in order:
    /// `position` ascending (operator-controlled), then an exact match over a
    /// `"*"`-only match, then route name ascending (routes are pre-sorted, so
    /// name-asc falls out of iteration order). Within the winning route, the
    /// strategy picks among the matching providers.
    pub fn resolve(&self, model: &str) -> RouteResolution {
        let mut best: Option<(usize, i32, bool)> = None; // (index, position, is_exact)
        for (ri, route) in self.routes.iter().enumerate() {
            let mut has_exact = false;
            let mut has_wild = false;
            for p in &route.providers {
                for g in &p.model_globs {
                    if g == model {
                        has_exact = true;
                    } else if g == "*" {
                        has_wild = true;
                    }
                }
            }
            if !has_exact && !has_wild {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, bp, be)) => {
                    route.position < bp || (route.position == bp && has_exact && !be)
                }
            };
            if better {
                best = Some((ri, route.position, has_exact));
            }
        }

        let Some((ri, _, is_exact)) = best else {
            return RouteResolution::NoRoute;
        };
        let route = &self.routes[ri];

        let matched: Vec<&CompiledProvider> = route
            .providers
            .iter()
            .filter(|p| p.model_globs.iter().any(|g| g == model || g == "*"))
            .collect();
        if matched.is_empty() {
            return RouteResolution::NoRoute;
        }

        let deps: Vec<Arc<Deployment>> = matched.iter().map(|p| p.deployment.clone()).collect();
        match select_from(&deps, &route.counter, route.strategy) {
            Some(idx) => RouteResolution::Routed(RouteResolved {
                backend_name: matched[idx].backend_name.clone(),
                model: model.to_string(),
                deployment: matched[idx].deployment.clone(),
                options: route.options.clone(),
            }),
            // All candidates are at their RPM limit. For an explicit (exact) match
            // the route is authoritative -> surface AllAtLimit (429). For a wildcard-
            // only match, a saturated catch-all must not 429 every model, so fall
            // through to the model_router / default backend instead.
            None if is_exact => RouteResolution::AllAtLimit,
            None => RouteResolution::NoRoute,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::db::{
        add_route_provider, insert_managed_backend, insert_route, ManagedBackendRow, RouteRow,
    };

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::admin::db::init_db(&c).unwrap();
        c
    }

    fn backend(id: &str, name: &str, rpm: Option<u32>) -> ManagedBackendRow {
        ManagedBackendRow {
            id: id.into(),
            name: name.into(),
            provider_id: "openai".into(),
            api_key: Some("sk".into()),
            api_base: None,
            deployment: None,
            api_version: None,
            project: None,
            region: None,
            aws_access_key_id: None,
            aws_secret_access_key: None,
            aws_session_token: None,
            rpm,
            tpm: None,
            enabled: true,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    fn route(id: &str, name: &str, strategy: &str, enabled: bool) -> RouteRow {
        RouteRow {
            id: id.into(),
            name: name.into(),
            description: None,
            strategy: strategy.into(),
            rpm: None,
            tpm: None,
            budget_usd: None,
            enabled,
            guardrail_mode: None,
            pxpipe_compress: None,
            pxpipe_models: None,
            redact_secrets: None,
            position: 0,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    #[test]
    fn exact_match_beats_wildcard_route() {
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "exact-be", None)).unwrap();
        insert_managed_backend(&c, &backend("b2", "wild-be", None)).unwrap();
        // "z-exact" sorts after "a-wild" — proves exact wins despite name order.
        insert_route(&c, &route("r1", "a-wild", "failover", true)).unwrap();
        insert_route(&c, &route("r2", "z-exact", "failover", true)).unwrap();
        add_route_provider(&c, "r1", "b2", &["*".into()], 0, true).unwrap();
        add_route_provider(&c, "r2", "b1", &["gpt-4o".into()], 0, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        match rr.resolve("gpt-4o") {
            RouteResolution::Routed(r) => assert_eq!(r.backend_name, "exact-be"),
            _ => panic!("expected exact route to win"),
        }
        // A model only the wildcard route serves still resolves to it.
        match rr.resolve("other-model") {
            RouteResolution::Routed(r) => assert_eq!(r.backend_name, "wild-be"),
            _ => panic!("expected wildcard route"),
        }
    }

    #[test]
    fn lower_position_wins_over_exact_match() {
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "wild-low", None)).unwrap();
        insert_managed_backend(&c, &backend("b2", "exact-high", None)).unwrap();
        // Wildcard route at position 0 outranks an exact-match route at position 5.
        let mut low = route("r1", "a-wild", "failover", true);
        low.position = 0;
        let mut high = route("r2", "z-exact", "failover", true);
        high.position = 5;
        insert_route(&c, &low).unwrap();
        insert_route(&c, &high).unwrap();
        add_route_provider(&c, "r1", "b1", &["*".into()], 0, true).unwrap();
        add_route_provider(&c, "r2", "b2", &["gpt-4o".into()], 0, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        match rr.resolve("gpt-4o") {
            RouteResolution::Routed(r) => assert_eq!(r.backend_name, "wild-low"),
            _ => panic!("lower position should win over exact match"),
        }
    }

    #[test]
    fn failover_sticks_then_falls_over_at_limit() {
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "primary", Some(1))).unwrap();
        insert_managed_backend(&c, &backend("b2", "secondary", None)).unwrap();
        insert_route(&c, &route("r1", "r", "failover", true)).unwrap();
        add_route_provider(&c, "r1", "b1", &["*".into()], 0, true).unwrap();
        add_route_provider(&c, "r1", "b2", &["*".into()], 1, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        // First request: priority-0 primary.
        assert!(
            matches!(rr.resolve("m"), RouteResolution::Routed(r) if r.backend_name == "primary")
        );
        // primary now at its RPM limit (1); failover to secondary.
        assert!(
            matches!(rr.resolve("m"), RouteResolution::Routed(r) if r.backend_name == "secondary")
        );
    }

    #[test]
    fn shared_backend_uses_one_rpm_counter_across_routes() {
        // Backend b1 (rpm=1) is referenced by two routes serving different models.
        // A single shared counter means one request exhausts it for BOTH routes.
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "shared", Some(1))).unwrap();
        insert_route(&c, &route("r1", "ra", "failover", true)).unwrap();
        insert_route(&c, &route("r2", "rb", "failover", true)).unwrap();
        add_route_provider(&c, "r1", "b1", &["ma".into()], 0, true).unwrap();
        add_route_provider(&c, "r2", "b1", &["mb".into()], 0, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        assert!(matches!(rr.resolve("ma"), RouteResolution::Routed(_)));
        // Same backend, different route/model: counter is shared, so it's at limit.
        assert!(matches!(rr.resolve("mb"), RouteResolution::AllAtLimit));
    }

    #[test]
    fn saturated_wildcard_route_falls_through() {
        // A wildcard-only route at its RPM limit must NOT 429 every model; it falls
        // through (NoRoute) so lower layers can serve. An exact match stays 429.
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "cap", Some(1))).unwrap();
        insert_route(&c, &route("r1", "wild", "failover", true)).unwrap();
        add_route_provider(&c, "r1", "b1", &["*".into()], 0, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        assert!(matches!(rr.resolve("m"), RouteResolution::Routed(_)));
        // Now saturated; wildcard match -> fall through instead of AllAtLimit.
        assert!(matches!(rr.resolve("m"), RouteResolution::NoRoute));
    }

    #[test]
    fn disabled_route_and_backend_excluded() {
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "be", None)).unwrap();
        insert_route(&c, &route("r1", "off", "failover", false)).unwrap();
        add_route_provider(&c, "r1", "b1", &["*".into()], 0, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        assert!(rr.is_empty());
        assert!(matches!(rr.resolve("m"), RouteResolution::NoRoute));

        // Enabled route but disabled backend -> route dropped (no live providers).
        let c2 = conn();
        insert_managed_backend(&c2, &backend("b2", "off-be", None).also_disabled()).unwrap();
        insert_route(&c2, &route("r2", "on", "failover", true)).unwrap();
        add_route_provider(&c2, "r2", "b2", &["*".into()], 0, true).unwrap();
        let rr2 = RouteRouter::build_from_db(&c2).unwrap();
        assert!(matches!(rr2.resolve("m"), RouteResolution::NoRoute));
    }

    #[test]
    fn options_propagate() {
        let c = conn();
        insert_managed_backend(&c, &backend("b1", "be", None)).unwrap();
        let mut r = route("r1", "r", "failover", true);
        r.redact_secrets = Some(true);
        r.guardrail_mode = Some("standard".into());
        insert_route(&c, &r).unwrap();
        add_route_provider(&c, "r1", "b1", &["*".into()], 0, true).unwrap();

        let rr = RouteRouter::build_from_db(&c).unwrap();
        match rr.resolve("m") {
            RouteResolution::Routed(res) => {
                assert_eq!(res.options.redact_secrets, Some(true));
                assert_eq!(res.options.guardrail_mode.as_deref(), Some("standard"));
                assert_eq!(res.model, "m");
            }
            _ => panic!("expected routed"),
        }
    }

    impl ManagedBackendRow {
        fn also_disabled(mut self) -> Self {
            self.enabled = false;
            self
        }
    }
}
