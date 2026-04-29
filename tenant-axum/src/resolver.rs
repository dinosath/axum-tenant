use tenant_core::error::TenantError;
use tenant_core::resolver::{ResolutionContext, TenantResolver};
use tenant_core::tenant::TenantId;

// ─── Header resolver ─────────────────────────────────────────────────

/// Resolves tenant from a request header (default: `X-Tenant-Id`).
///
/// Resolves from a configurable request header.
#[derive(Debug, Clone)]
pub struct HeaderTenantResolver {
    header_name: String,
}

impl HeaderTenantResolver {
    pub fn new(header_name: impl Into<String>) -> Self {
        Self {
            header_name: header_name.into(),
        }
    }
}

impl Default for HeaderTenantResolver {
    fn default() -> Self {
        Self::new("X-Tenant-Id")
    }
}

impl TenantResolver for HeaderTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        match ctx.header(&self.header_name) {
            Some(value) => Ok(TenantId::new(value)),
            None => Ok(None),
        }
    }

    fn name(&self) -> &str {
        "HeaderTenantResolver"
    }
}

// ─── Path resolver ───────────────────────────────────────────────────

/// Resolves tenant from a URL path segment.
/// E.g. `/{tenant}/api/resource` with `segment_index = 0`.
#[derive(Debug, Clone)]
pub struct PathTenantResolver {
    segment_index: usize,
}

impl PathTenantResolver {
    pub fn new(segment_index: usize) -> Self {
        Self { segment_index }
    }
}

impl Default for PathTenantResolver {
    fn default() -> Self {
        Self::new(0)
    }
}

impl TenantResolver for PathTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        let path = match ctx.path() {
            Some(p) => p,
            None => return Ok(None),
        };
        let segment = path
            .split('/')
            .filter(|s| !s.is_empty())
            .nth(self.segment_index);
        match segment {
            Some(s) => Ok(TenantId::new(s)),
            None => Ok(None),
        }
    }

    fn name(&self) -> &str {
        "PathTenantResolver"
    }
}

// ─── Subdomain resolver ──────────────────────────────────────────────

/// Resolves tenant from the `Host` header subdomain.
/// E.g. `tenant1.example.com` → `tenant1`.
#[derive(Debug, Clone)]
pub struct SubdomainTenantResolver;

impl TenantResolver for SubdomainTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        let host = match ctx.header("host") {
            Some(h) => h,
            None => return Ok(None),
        };
        // Strip port
        let host = host.split(':').next().unwrap_or(host);
        let subdomain = host.split('.').next();
        match subdomain {
            Some(s) => Ok(TenantId::new(s)),
            None => Ok(None),
        }
    }

    fn name(&self) -> &str {
        "SubdomainTenantResolver"
    }
}

// ─── Query param resolver ────────────────────────────────────────────

/// Resolves tenant from a query parameter (default: `tenant_id`).
#[derive(Debug, Clone)]
pub struct QueryParamTenantResolver {
    param_name: String,
}

impl QueryParamTenantResolver {
    pub fn new(param_name: impl Into<String>) -> Self {
        Self {
            param_name: param_name.into(),
        }
    }
}

impl Default for QueryParamTenantResolver {
    fn default() -> Self {
        Self::new("tenant_id")
    }
}

impl TenantResolver for QueryParamTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        let query = match ctx.query() {
            Some(q) => q,
            None => return Ok(None),
        };
        // Simple query string parsing (no external dependency)
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                if key == self.param_name {
                    return Ok(TenantId::new(value));
                }
            }
        }
        Ok(None)
    }

    fn name(&self) -> &str {
        "QueryParamTenantResolver"
    }
}

/// A resolver that provides a hardcoded default/fallback tenant.
/// Provides a hardcoded fallback tenant.
#[derive(Debug, Clone)]
pub struct DefaultTenantResolver {
    default_tenant: TenantId,
}

impl DefaultTenantResolver {
    pub fn new(tenant_id: impl Into<String>) -> Option<Self> {
        TenantId::new(tenant_id).map(|id| Self { default_tenant: id })
    }
}

impl TenantResolver for DefaultTenantResolver {
    fn resolve(&self, _ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        Ok(Some(self.default_tenant.clone()))
    }

    fn name(&self) -> &str {
        "DefaultTenantResolver"
    }
}

// ─── Cookie resolver ─────────────────────────────────────────────────

/// Resolves tenant from a cookie (default: `tenant_cookie`).
///
/// Parses the `Cookie` header to find the named cookie. Analogous to
/// the `Cookie` strategy in `HttpTenantConfig`.
#[derive(Debug, Clone)]
pub struct CookieTenantResolver {
    cookie_name: String,
}

impl CookieTenantResolver {
    pub fn new(cookie_name: impl Into<String>) -> Self {
        Self {
            cookie_name: cookie_name.into(),
        }
    }
}

impl Default for CookieTenantResolver {
    fn default() -> Self {
        Self::new("tenant_cookie")
    }
}

impl TenantResolver for CookieTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        let cookie_header = match ctx.header("cookie") {
            Some(c) => c,
            None => return Ok(None),
        };
        // Parse `Cookie: name1=val1; name2=val2`
        for pair in cookie_header.split(';') {
            let pair = pair.trim();
            if let Some((key, value)) = pair.split_once('=') {
                if key.trim() == self.cookie_name {
                    return Ok(TenantId::new(value.trim()));
                }
            }
        }
        Ok(None)
    }

    fn name(&self) -> &str {
        "CookieTenantResolver"
    }
}

// ─── JWT resolver ────────────────────────────────────────────────────

/// Resolves tenant from a JWT Bearer token claim.
///
/// Decodes the JWT payload and extracts the configured claim. By default,
/// signature verification is **disabled** (the assumption is that a prior
/// authentication middleware has already verified the token). Use
/// [`JwtTenantResolver::with_validation`] to supply custom validation
/// (e.g. with a decoding key for signature verification).
///
/// Corresponds to the `Jwt` strategy in `HttpTenantConfig`.
///
/// Default claim name: `tenant`.
#[derive(Debug, Clone)]
pub struct JwtTenantResolver {
    claim_name: String,
    validation: jsonwebtoken::Validation,
}

impl JwtTenantResolver {
    pub fn new(claim_name: impl Into<String>) -> Self {
        let mut validation = jsonwebtoken::Validation::default();
        validation.insecure_disable_signature_validation();
        validation.validate_aud = false;
        validation.validate_exp = false;
        validation.required_spec_claims.clear();
        Self {
            claim_name: claim_name.into(),
            validation,
        }
    }

    /// Create a resolver with custom `jsonwebtoken::Validation` settings.
    /// Useful for enabling signature verification, audience checks, etc.
    pub fn with_validation(
        claim_name: impl Into<String>,
        validation: jsonwebtoken::Validation,
    ) -> Self {
        Self {
            claim_name: claim_name.into(),
            validation,
        }
    }
}

impl Default for JwtTenantResolver {
    fn default() -> Self {
        Self::new("tenant")
    }
}

impl TenantResolver for JwtTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        let auth = match ctx.header("authorization") {
            Some(a) => a,
            None => return Ok(None),
        };
        let token = match auth
            .strip_prefix("Bearer ")
            .or_else(|| auth.strip_prefix("bearer "))
        {
            Some(t) => t.trim(),
            None => return Ok(None),
        };

        let key = jsonwebtoken::DecodingKey::from_secret(&[]);
        let token_data: jsonwebtoken::TokenData<serde_json::Value> =
            jsonwebtoken::decode(token, &key, &self.validation)
                .map_err(|e| TenantError::InvalidTenant(format!("Failed to decode JWT: {e}")))?;

        let claim_value = token_data.claims.get(&self.claim_name).map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        });

        match claim_value {
            Some(v) => Ok(TenantId::new(v)),
            None => Ok(None),
        }
    }

    fn name(&self) -> &str {
        "JwtTenantResolver"
    }
}
