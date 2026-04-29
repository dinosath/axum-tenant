use crate::tenant::MultiTenancyStrategy;
use serde::{Deserialize, Serialize};

/// Top-level multi-tenancy configuration.
///
/// Controls whether multi-tenancy is active and which isolation strategy
/// is used.
///
/// ```rust
/// use tenant_core::config::TenantConfig;
/// use tenant_core::MultiTenancyStrategy;
///
/// let config = TenantConfig::builder()
///     .enabled(true)
///     .strategy(MultiTenancyStrategy::Database)
///     .default_tenant("public")
///     .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// Whether multi-tenancy is enabled. When `false`, the middleware is a
    /// no-op pass-through.
    pub enabled: bool,

    /// The multi-tenancy isolation strategy.
    pub strategy: MultiTenancyStrategy,

    /// Fallback tenant identifier used when no resolver can determine the
    /// tenant.
    pub default_tenant: Option<String>,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: MultiTenancyStrategy::Discriminator,
            default_tenant: None,
        }
    }
}

impl TenantConfig {
    pub fn builder() -> TenantConfigBuilder {
        TenantConfigBuilder::default()
    }
}

#[derive(Debug, Default)]
pub struct TenantConfigBuilder {
    enabled: Option<bool>,
    strategy: Option<MultiTenancyStrategy>,
    default_tenant: Option<String>,
}

impl TenantConfigBuilder {
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    pub fn strategy(mut self, strategy: MultiTenancyStrategy) -> Self {
        self.strategy = Some(strategy);
        self
    }

    pub fn default_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.default_tenant = Some(tenant.into());
        self
    }

    pub fn build(self) -> TenantConfig {
        let defaults = TenantConfig::default();
        TenantConfig {
            enabled: self.enabled.unwrap_or(defaults.enabled),
            strategy: self.strategy.unwrap_or(defaults.strategy),
            default_tenant: self.default_tenant.or(defaults.default_tenant),
        }
    }
}
