//! OIDC-based tenant resolver with proper token validation.
//!
//! Requires the `oidc` feature flag.

use jsonwebtoken::{DecodingKey, TokenData, Validation};
use openidconnect::core::{CoreJsonWebKey, CoreJsonWebKeySet, CoreProviderMetadata};
use openidconnect::IssuerUrl;
use serde_json::Value;
use std::sync::Arc;
use tenant_core::error::TenantError;
use tenant_core::resolver::{ResolutionContext, TenantResolver};
use tenant_core::tenant::TenantId;
use tokio::sync::RwLock;

/// Resolves tenant from a JWT Bearer token after performing full OIDC
/// validation (signature verification via JWKS fetched from the provider's
/// discovery endpoint).
///
/// Construct with [`OidcTenantResolver::discover`] which performs OIDC
/// discovery to obtain the JWKS. The JWKS is cached and can be refreshed
/// with [`OidcTenantResolver::refresh_jwks`].
///
/// # Example
///
/// ```rust,no_run
/// # async fn example() {
/// use tenant_axum::oidc::OidcTenantResolver;
///
/// let resolver = OidcTenantResolver::discover(
///     "https://auth.example.com/realms/master",
///     "tenant",
/// )
/// .expect("OIDC discovery failed");
/// # }
/// ```
#[derive(Clone)]
pub struct OidcTenantResolver {
    claim_name: String,
    issuer: String,
    jwks: Arc<RwLock<CoreJsonWebKeySet>>,
    audience: Option<String>,
}

impl OidcTenantResolver {
    /// Perform OIDC discovery and create a resolver.
    ///
    /// `issuer_url` is the base URL of the OIDC provider (e.g.
    /// `https://auth.example.com/realms/master`).
    ///
    /// `claim_name` is the JWT claim to extract as the tenant identifier.
    pub fn discover(issuer_url: &str, claim_name: impl Into<String>) -> Result<Self, TenantError> {
        let issuer = IssuerUrl::new(issuer_url.to_string())
            .map_err(|e| TenantError::ConfigError(format!("Invalid issuer URL: {e}")))?;

        let http_client = reqwest::blocking::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| TenantError::ConfigError(format!("Failed to build HTTP client: {e}")))?;

        let metadata = CoreProviderMetadata::discover(&issuer, &http_client)
            .map_err(|e| TenantError::ConfigError(format!("OIDC discovery failed: {e}")))?;

        let jwks_url = metadata.jwks_uri().clone();
        let jwks = fetch_jwks(&http_client, &jwks_url)?;

        Ok(Self {
            claim_name: claim_name.into(),
            issuer: issuer_url.to_string(),
            jwks: Arc::new(RwLock::new(jwks)),
            audience: None,
        })
    }

    /// Set an expected audience (`aud`) claim for validation.
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// Refresh the cached JWKS from the provider (call periodically or on
    /// key-rotation errors).
    pub fn refresh_jwks(&self) -> Result<(), TenantError> {
        let issuer = IssuerUrl::new(self.issuer.clone())
            .map_err(|e| TenantError::ConfigError(format!("Invalid issuer URL: {e}")))?;

        let http_client = reqwest::blocking::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| TenantError::ConfigError(format!("Failed to build HTTP client: {e}")))?;

        let metadata = CoreProviderMetadata::discover(&issuer, &http_client)
            .map_err(|e| TenantError::ConfigError(format!("OIDC discovery failed: {e}")))?;

        let jwks_url = metadata.jwks_uri().clone();
        let new_jwks = fetch_jwks(&http_client, &jwks_url)?;

        // Use blocking_write since this method is sync
        let mut guard = self.jwks.blocking_write();
        *guard = new_jwks;
        Ok(())
    }
}

impl TenantResolver for OidcTenantResolver {
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

        let header = jsonwebtoken::decode_header(token)
            .map_err(|e| TenantError::InvalidTenant(format!("Invalid JWT header: {e}")))?;

        let jwks = self.jwks.blocking_read();
        let decoding_key = find_decoding_key(&jwks, header.kid.as_deref())?;

        let mut validation = Validation::new(header.alg);
        validation.validate_exp = true;
        validation.set_issuer(&[&self.issuer]);
        if let Some(ref aud) = self.audience {
            validation.set_audience(&[aud]);
        } else {
            validation.validate_aud = false;
        }

        let token_data: TokenData<Value> = jsonwebtoken::decode(token, &decoding_key, &validation)
            .map_err(|e| TenantError::InvalidTenant(format!("JWT validation failed: {e}")))?;

        let claim_value = token_data.claims.get(&self.claim_name).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });

        match claim_value {
            Some(v) => Ok(TenantId::new(v)),
            None => Ok(None),
        }
    }

    fn name(&self) -> &str {
        "OidcTenantResolver"
    }
}

fn fetch_jwks(
    http_client: &reqwest::blocking::Client,
    url: &openidconnect::JsonWebKeySetUrl,
) -> Result<CoreJsonWebKeySet, TenantError> {
    let response = http_client
        .get(url.url().as_str())
        .send()
        .map_err(|e| TenantError::ConfigError(format!("Failed to fetch JWKS: {e}")))?;

    let jwks: CoreJsonWebKeySet = response
        .json()
        .map_err(|e| TenantError::ConfigError(format!("Failed to parse JWKS: {e}")))?;

    Ok(jwks)
}

fn find_decoding_key(
    jwks: &CoreJsonWebKeySet,
    kid: Option<&str>,
) -> Result<DecodingKey, TenantError> {
    use openidconnect::JsonWebKey;

    let keys = jwks.keys();

    let key: &CoreJsonWebKey = if let Some(kid) = kid {
        keys.iter()
            .find(|k| k.key_id().map(|id| id.as_str()) == Some(kid))
            .ok_or_else(|| {
                TenantError::InvalidTenant(format!("No matching key found for kid: {kid}"))
            })?
    } else {
        keys.first()
            .ok_or_else(|| TenantError::InvalidTenant("JWKS is empty, no keys available".into()))?
    };

    let jwk_json = serde_json::to_value(key)
        .map_err(|e| TenantError::InvalidTenant(format!("Failed to serialize JWK: {e}")))?;

    DecodingKey::from_jwk(&serde_json::from_value(jwk_json).map_err(|e| {
        TenantError::InvalidTenant(format!("Failed to parse JWK for decoding: {e}"))
    })?)
    .map_err(|e| TenantError::InvalidTenant(format!("Failed to create decoding key: {e}")))
}
