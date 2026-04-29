use tenant_core::config::TenantConfig;
use tenant_core::context::{DefaultTenantContext, TenantContext};
use tenant_core::error::TenantError;
use tenant_core::resolver::{CompositeTenantResolver, ResolutionContext, TenantResolver};
use tenant_core::tenant::{MultiTenancyStrategy, TenantId};

// ─── TenantId ────────────────────────────────────────────────────────

#[test]
fn tenant_id_valid() {
    let id = TenantId::new("acme").unwrap();
    assert_eq!(id.as_str(), "acme");
    assert_eq!(id.to_string(), "acme");
    assert_eq!(id.as_ref(), "acme");
}

#[test]
fn tenant_id_rejects_empty() {
    assert!(TenantId::new("").is_none());
}

#[test]
fn tenant_id_rejects_whitespace_only() {
    assert!(TenantId::new("   ").is_none());
    assert!(TenantId::new("\t\n").is_none());
}

#[test]
fn tenant_id_preserves_whitespace_around_content() {
    // Whitespace-padded but contains content — valid per current impl
    let id = TenantId::new("  acme  ").unwrap();
    assert_eq!(id.as_str(), "  acme  ");
}

#[test]
fn tenant_id_into_inner() {
    let id = TenantId::new("acme").unwrap();
    let s: String = id.into_inner();
    assert_eq!(s, "acme");
}

#[test]
fn tenant_id_equality_and_hash() {
    use std::collections::HashSet;
    let a = TenantId::new("t1").unwrap();
    let b = TenantId::new("t1").unwrap();
    let c = TenantId::new("t2").unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);

    let mut set = HashSet::new();
    set.insert(a);
    assert!(set.contains(&b));
    assert!(!set.contains(&c));
}

#[test]
fn tenant_id_serde_roundtrip() {
    let id = TenantId::new("acme").unwrap();
    let json = serde_json::to_string(&id).unwrap();
    let deserialized: TenantId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, deserialized);
}

// ─── MultiTenancyStrategy ────────────────────────────────────────────

#[test]
fn strategy_serde_roundtrip() {
    for strategy in [
        MultiTenancyStrategy::Database,
        MultiTenancyStrategy::Schema,
        MultiTenancyStrategy::Discriminator,
    ] {
        let json = serde_json::to_string(&strategy).unwrap();
        let deserialized: MultiTenancyStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(strategy, deserialized);
    }
}

// ─── TenantError ─────────────────────────────────────────────────────

#[test]
fn tenant_error_display_messages() {
    assert_eq!(
        TenantError::MissingTenant.to_string(),
        "No tenant identifier found in the request"
    );
    assert_eq!(
        TenantError::InvalidTenant("bad".into()).to_string(),
        "Invalid tenant identifier: bad"
    );
    assert_eq!(
        TenantError::TenantNotFound("t1".into()).to_string(),
        "Tenant not found: t1"
    );
    assert_eq!(
        TenantError::ConnectionError("timeout".into()).to_string(),
        "Database connection error: timeout"
    );
    assert_eq!(
        TenantError::SchemaError("fail".into()).to_string(),
        "Schema switching error: fail"
    );
    assert_eq!(
        TenantError::ConfigError("bad".into()).to_string(),
        "Configuration error: bad"
    );
    assert_eq!(TenantError::Other("misc".into()).to_string(), "misc");
}

// ─── DefaultTenantContext ────────────────────────────────────────────

#[test]
fn context_default_has_no_tenant() {
    let ctx = DefaultTenantContext::new();
    assert!(ctx.tenant_id().is_none());
}

#[test]
fn context_with_tenant() {
    let id = TenantId::new("acme").unwrap();
    let ctx = DefaultTenantContext::with_tenant(id.clone());
    assert_eq!(ctx.tenant_id(), Some(&id));
}

#[test]
fn context_set_and_clear() {
    let mut ctx = DefaultTenantContext::new();
    let id = TenantId::new("acme").unwrap();

    ctx.set_tenant_id(id.clone());
    assert_eq!(ctx.tenant_id(), Some(&id));

    ctx.clear();
    assert!(ctx.tenant_id().is_none());
}

#[test]
fn context_overwrite_tenant() {
    let mut ctx = DefaultTenantContext::with_tenant(TenantId::new("t1").unwrap());
    let t2 = TenantId::new("t2").unwrap();
    ctx.set_tenant_id(t2.clone());
    assert_eq!(ctx.tenant_id(), Some(&t2));
}

// ─── ResolutionContext (mock) & Resolvers ─────────────────────────────

struct MockContext {
    headers: Vec<(String, String)>,
    path: Option<String>,
    query: Option<String>,
}

impl MockContext {
    fn empty() -> Self {
        Self {
            headers: vec![],
            path: None,
            query: None,
        }
    }

    #[allow(dead_code)]
    fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    #[allow(dead_code)]
    fn with_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }

    #[allow(dead_code)]
    fn with_query(mut self, query: &str) -> Self {
        self.query = Some(query.to_string());
        self
    }
}

impl ResolutionContext for MockContext {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }
}

// ─── CompositeTenantResolver ─────────────────────────────────────────

/// A resolver that always returns None.
struct NoneResolver;
impl TenantResolver for NoneResolver {
    fn resolve(&self, _ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        Ok(None)
    }
}

/// A resolver that always returns a fixed tenant.
struct FixedResolver(TenantId);
impl TenantResolver for FixedResolver {
    fn resolve(&self, _ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        Ok(Some(self.0.clone()))
    }
}

/// A resolver that always errors.
struct ErrorResolver;
impl TenantResolver for ErrorResolver {
    fn resolve(&self, _ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        Err(TenantError::InvalidTenant("always fails".into()))
    }
}

#[test]
fn composite_empty_returns_none() {
    let resolver = CompositeTenantResolver::new();
    let ctx = MockContext::empty();
    assert!(resolver.resolve(&ctx).unwrap().is_none());
}

#[test]
fn composite_first_match_wins() {
    let resolver = CompositeTenantResolver::new()
        .add(FixedResolver(TenantId::new("first").unwrap()))
        .add(FixedResolver(TenantId::new("second").unwrap()));
    let ctx = MockContext::empty();
    assert_eq!(resolver.resolve(&ctx).unwrap().unwrap().as_str(), "first");
}

#[test]
fn composite_skips_none_resolvers() {
    let resolver = CompositeTenantResolver::new()
        .add(NoneResolver)
        .add(NoneResolver)
        .add(FixedResolver(TenantId::new("third").unwrap()));
    let ctx = MockContext::empty();
    assert_eq!(resolver.resolve(&ctx).unwrap().unwrap().as_str(), "third");
}

#[test]
fn composite_error_propagates_immediately() {
    let resolver = CompositeTenantResolver::new()
        .add(NoneResolver)
        .add(ErrorResolver)
        .add(FixedResolver(TenantId::new("never").unwrap()));
    let ctx = MockContext::empty();
    assert!(resolver.resolve(&ctx).is_err());
}

#[test]
fn composite_all_none_returns_none() {
    let resolver = CompositeTenantResolver::new()
        .add(NoneResolver)
        .add(NoneResolver);
    let ctx = MockContext::empty();
    assert!(resolver.resolve(&ctx).unwrap().is_none());
}

#[test]
fn composite_implements_tenant_resolver_trait() {
    // CompositeTenantResolver itself implements TenantResolver, so it can be
    // nested inside another composite.
    let inner = CompositeTenantResolver::new()
        .add(FixedResolver(TenantId::new("nested").unwrap()));
    let outer = CompositeTenantResolver::new().add(inner);
    let ctx = MockContext::empty();
    assert_eq!(
        outer.resolve(&ctx).unwrap().unwrap().as_str(),
        "nested"
    );
}

// ─── TenantConfig ────────────────────────────────────────────────────

#[test]
fn config_default_values() {
    let config = TenantConfig::default();
    assert!(config.enabled);
    assert_eq!(config.strategy, MultiTenancyStrategy::Discriminator);
    assert!(config.default_tenant.is_none());
}

#[test]
fn config_builder() {
    let config = TenantConfig::builder()
        .enabled(false)
        .strategy(MultiTenancyStrategy::Database)
        .default_tenant("public")
        .build();
    assert!(!config.enabled);
    assert_eq!(config.strategy, MultiTenancyStrategy::Database);
    assert_eq!(config.default_tenant.as_deref(), Some("public"));
}

#[test]
fn config_builder_partial_uses_defaults() {
    let config = TenantConfig::builder()
        .strategy(MultiTenancyStrategy::Schema)
        .build();
    assert!(config.enabled); // default
    assert_eq!(config.strategy, MultiTenancyStrategy::Schema);
    assert!(config.default_tenant.is_none()); // default
}

#[test]
fn config_serde_roundtrip() {
    let config = TenantConfig::builder()
        .enabled(true)
        .strategy(MultiTenancyStrategy::Database)
        .default_tenant("test")
        .build();
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: TenantConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.enabled, config.enabled);
    assert_eq!(deserialized.strategy, config.strategy);
    assert_eq!(deserialized.default_tenant, config.default_tenant);
}
