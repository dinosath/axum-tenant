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
/// Decodes the JWT payload (base64) **without verifying the signature** —
/// signature verification is expected to happen in a prior authentication
/// middleware. The resolver extracts the configured claim from the payload.
///
/// Corresponds to the `Jwt` strategy in `HttpTenantConfig`.
///
/// Default claim name: `tenant`.
#[derive(Debug, Clone)]
pub struct JwtTenantResolver {
    claim_name: String,
}

impl JwtTenantResolver {
    pub fn new(claim_name: impl Into<String>) -> Self {
        Self {
            claim_name: claim_name.into(),
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
        let token = match auth.strip_prefix("Bearer ").or_else(|| auth.strip_prefix("bearer ")) {
            Some(t) => t.trim(),
            None => return Ok(None),
        };
        // JWT has 3 parts: header.payload.signature
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Ok(None);
        }
        let payload = parts[1];
        // Decode base64url → JSON
        let decoded = base64url_decode(payload).map_err(|e| {
            TenantError::InvalidTenant(format!("Failed to decode JWT payload: {e}"))
        })?;
        // Simple JSON value extraction without pulling in serde_json as a dep.
        // Look for `"claim_name":"value"` or `"claim_name": "value"`.
        let claim_value = extract_json_string_value(&decoded, &self.claim_name);
        match claim_value {
            Some(v) => Ok(TenantId::new(v)),
            None => Ok(None),
        }
    }

    fn name(&self) -> &str {
        "JwtTenantResolver"
    }
}

/// Minimal base64url decoder (no padding required).
fn base64url_decode(input: &str) -> Result<String, String> {
    // Replace URL-safe chars with standard base64 chars
    let mut b64 = input.replace('-', "+").replace('_', "/");
    // Add padding
    match b64.len() % 4 {
        2 => b64.push_str("=="),
        3 => b64.push('='),
        0 => {}
        _ => return Err("Invalid base64url length".into()),
    }
    // Decode
    let bytes = base64_decode_simple(&b64)?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Minimal base64 decoder (no external crate).
fn base64_decode_simple(input: &str) -> Result<Vec<u8>, String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in input.as_bytes() {
        if byte == b'=' {
            break;
        }
        let val = TABLE
            .iter()
            .position(|&c| c == byte)
            .ok_or_else(|| format!("Invalid base64 character: {}", byte as char))?
            as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(output)
}

/// Extract a string value from a JSON object string by key.
/// Very simple parser — handles `"key":"value"` and `"key": "value"`.
fn extract_json_string_value(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let idx = json.find(&search)?;
    let rest = &json[idx + search.len()..];
    // Skip optional whitespace and colon
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    // Expect a quoted string value
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
