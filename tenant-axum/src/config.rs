use crate::resolver::{
    CookieTenantResolver, DefaultTenantResolver, HeaderTenantResolver, JwtTenantResolver,
    PathTenantResolver, QueryParamTenantResolver, SubdomainTenantResolver,
};
use serde::{Deserialize, Serialize};
use tenant_core::CompositeTenantResolver;

/// HTTP-level tenant resolution strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpTenantStrategy {
    /// Resolve from a request header (default: `X-Tenant-Id`).
    Header,
    /// Resolve from a JWT claim.
    Jwt,
    /// Resolve from a cookie.
    Cookie,
    /// Resolve from a URL path segment.
    Path,
    /// Resolve from a query parameter.
    Query,
    /// Resolve from the `Host` header subdomain.
    Subdomain,
}

/// HTTP tenant resolution configuration.
///
/// # Example
///
/// ```rust
/// use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
///
/// let config = HttpTenantConfig::builder()
///     .enabled(true)
///     .strategy(HttpTenantStrategy::Header)
///     .strategy(HttpTenantStrategy::Jwt)
///     .header_name("X-Tenant")
///     .jwt_claim_name("tenant_id")
///     .default_tenant("public")
///     .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpTenantConfig {
    /// Whether HTTP tenant resolution is enabled.
    pub enabled: bool,

    /// Ordered list of resolution strategies. Tried in order; first match
    /// wins.
    pub strategies: Vec<HttpTenantStrategy>,

    /// Header name for the `Header` strategy.
    pub header_name: String,

    /// JWT claim name for the `Jwt` strategy.
    pub jwt_claim_name: String,

    /// Whether to validate the JWT (signature, expiry) when using the `Jwt`
    /// strategy. When `false` (default), the token payload is base64-decoded
    /// without cryptographic verification — rely on upstream auth middleware.
    /// When `true`, `jsonwebtoken` performs full validation (requires a
    /// crypto provider feature like `aws_lc_rs`).
    pub jwt_validate: bool,

    /// Cookie name for the `Cookie` strategy.
    pub cookie_name: String,

    /// Path segment index for the `Path` strategy (0-based, after splitting
    /// on `/`).
    pub path_segment_index: usize,

    /// Query parameter name for the `Query` strategy.
    pub query_param_name: String,

    /// Fallback tenant when no resolver matches. If `None`, a missing
    /// tenant is an error.
    pub default_tenant: Option<String>,
}

impl Default for HttpTenantConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategies: vec![HttpTenantStrategy::Header],
            header_name: "X-Tenant-Id".into(),
            jwt_claim_name: "tenant".into(),
            jwt_validate: false,
            cookie_name: "tenant_cookie".into(),
            path_segment_index: 0,
            query_param_name: "tenant_id".into(),
            default_tenant: None,
        }
    }
}

impl HttpTenantConfig {
    pub fn builder() -> HttpTenantConfigBuilder {
        HttpTenantConfigBuilder::default()
    }
}

/// Builder for [`HttpTenantConfig`].
#[derive(Debug, Default)]
pub struct HttpTenantConfigBuilder {
    enabled: Option<bool>,
    strategies: Vec<HttpTenantStrategy>,
    header_name: Option<String>,
    jwt_claim_name: Option<String>,
    jwt_validate: Option<bool>,
    cookie_name: Option<String>,
    path_segment_index: Option<usize>,
    query_param_name: Option<String>,
    default_tenant: Option<String>,
}

impl HttpTenantConfigBuilder {
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Add a resolution strategy. Can be called multiple times to chain
    /// strategies (tried in order).
    pub fn strategy(mut self, strategy: HttpTenantStrategy) -> Self {
        self.strategies.push(strategy);
        self
    }

    /// Replace all strategies at once.
    pub fn strategies(mut self, strategies: Vec<HttpTenantStrategy>) -> Self {
        self.strategies = strategies;
        self
    }

    pub fn header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = Some(name.into());
        self
    }

    pub fn jwt_claim_name(mut self, name: impl Into<String>) -> Self {
        self.jwt_claim_name = Some(name.into());
        self
    }

    /// Whether to validate the JWT cryptographically.
    ///
    /// - `false` (default): payload is base64-decoded without signature
    ///   verification. Suitable when an upstream auth middleware has already
    ///   validated the token.
    /// - `true`: uses `jsonwebtoken` for full validation (signature, expiry).
    pub fn jwt_validate(mut self, validate: bool) -> Self {
        self.jwt_validate = Some(validate);
        self
    }

    pub fn cookie_name(mut self, name: impl Into<String>) -> Self {
        self.cookie_name = Some(name.into());
        self
    }

    pub fn path_segment_index(mut self, index: usize) -> Self {
        self.path_segment_index = Some(index);
        self
    }

    pub fn query_param_name(mut self, name: impl Into<String>) -> Self {
        self.query_param_name = Some(name.into());
        self
    }

    pub fn default_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.default_tenant = Some(tenant.into());
        self
    }

    pub fn build(self) -> HttpTenantConfig {
        let defaults = HttpTenantConfig::default();
        HttpTenantConfig {
            enabled: self.enabled.unwrap_or(defaults.enabled),
            strategies: if self.strategies.is_empty() {
                defaults.strategies
            } else {
                self.strategies
            },
            header_name: self.header_name.unwrap_or(defaults.header_name),
            jwt_claim_name: self.jwt_claim_name.unwrap_or(defaults.jwt_claim_name),
            jwt_validate: self.jwt_validate.unwrap_or(defaults.jwt_validate),
            cookie_name: self.cookie_name.unwrap_or(defaults.cookie_name),
            path_segment_index: self
                .path_segment_index
                .unwrap_or(defaults.path_segment_index),
            query_param_name: self.query_param_name.unwrap_or(defaults.query_param_name),
            default_tenant: self.default_tenant.or(defaults.default_tenant),
        }
    }
}

impl HttpTenantConfig {
    /// Build a [`CompositeTenantResolver`] from this configuration.
    ///
    /// Each enabled strategy is added in order. If `default_tenant` is set,
    /// a [`DefaultTenantResolver`] is appended as the final fallback.
    ///
    /// ```rust
    /// use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
    ///
    /// let config = HttpTenantConfig::builder()
    ///     .strategy(HttpTenantStrategy::Header)
    ///     .strategy(HttpTenantStrategy::Cookie)
    ///     .default_tenant("public")
    ///     .build();
    ///
    /// let resolver = config.into_resolver();
    /// ```
    pub fn into_resolver(&self) -> CompositeTenantResolver {
        let mut resolver = CompositeTenantResolver::new();

        for strategy in &self.strategies {
            resolver = match strategy {
                HttpTenantStrategy::Header => {
                    resolver.add(HeaderTenantResolver::new(&self.header_name))
                }
                HttpTenantStrategy::Jwt => resolver.add(JwtTenantResolver::with_validate(
                    &self.jwt_claim_name,
                    self.jwt_validate,
                )),
                HttpTenantStrategy::Cookie => {
                    resolver.add(CookieTenantResolver::new(&self.cookie_name))
                }
                HttpTenantStrategy::Path => {
                    resolver.add(PathTenantResolver::new(self.path_segment_index))
                }
                HttpTenantStrategy::Query => {
                    resolver.add(QueryParamTenantResolver::new(&self.query_param_name))
                }
                HttpTenantStrategy::Subdomain => resolver.add(SubdomainTenantResolver),
            };
        }

        if let Some(ref default) = self.default_tenant {
            if let Some(fallback) = DefaultTenantResolver::new(default) {
                resolver = resolver.add(fallback);
            }
        }

        resolver
    }
}
