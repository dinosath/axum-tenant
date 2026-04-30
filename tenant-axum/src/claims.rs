//! Claims-based tenant resolution.
//!
//! This module provides the **recommended** way to extract tenant identity
//! from JWT tokens: rely on an upstream authentication middleware to validate
//! the token and place the decoded claims into request extensions, then use
//! [`TenantLayer::with_claims`](crate::middleware::TenantLayer::with_claims)
//! to resolve the tenant from those pre-validated claims.
//!
//! # Architecture
//!
//! ```text
//! Request
//!   │
//!   ▼
//! ┌─────────────────────────┐
//! │  Auth Middleware         │  ← validates JWT (signature, expiry, etc.)
//! │  (tower-http, custom)   │     inserts JwtClaims into extensions
//! └───────────┬─────────────┘
//!             ▼
//! ┌─────────────────────────┐
//! │  TenantLayer            │  ← reads tenant from JwtClaims in extensions
//! │  (claims from config)   │     falls back to resolver chain if absent
//! └───────────┬─────────────┘
//!             ▼
//!         Handler
//! ```
//!
//! # Configuration-driven usage (recommended)
//!
//! When using [`HttpTenantConfig`](crate::config::HttpTenantConfig) with the
//! `Jwt` strategy, claims-based resolution is enabled automatically using
//! the configured `jwt_claim_name`:
//!
//! ```rust,ignore
//! use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
//! use tenant_axum::TenantLayer;
//!
//! let config = HttpTenantConfig::builder()
//!     .strategy(HttpTenantStrategy::Jwt)
//!     .jwt_claim_name("tenant_id")  // claim to extract tenant from
//!     .build();
//!
//! let layer = TenantLayer::from_config(&config);
//! // Upstream auth middleware inserts JwtClaims → tenant resolved from "tenant_id" claim
//! // No JwtClaims present → falls back to resolver chain
//! ```
//!
//! # Manual usage
//!
//! ```rust,ignore
//! use tenant_axum::claims::JwtClaims;
//! use tenant_axum::{TenantLayer, HeaderTenantResolver};
//!
//! // Your auth middleware inserts JwtClaims into extensions:
//! // parts.extensions.insert(JwtClaims::new(decoded_claims));
//!
//! let layer = TenantLayer::new(HeaderTenantResolver::default())
//!     .with_claims("tenant_id");
//!
//! // With JwtClaims present → tenant resolved from claims
//! // Without JwtClaims     → falls back to HeaderTenantResolver
//! ```

use axum::extract::FromRequestParts;
use http::request::Parts;
use serde_json::Value;
use tenant_core::error::TenantError;

/// Pre-validated JWT claims inserted into request extensions by an upstream
/// auth middleware.
///
/// This is the bridge between your authentication layer and the tenant
/// resolution layer. Your auth middleware validates the JWT (checking
/// signature, expiry, audience, etc.) and then inserts this into request
/// extensions:
///
/// ```rust,ignore
/// use tenant_axum::claims::JwtClaims;
///
/// // In your auth middleware:
/// let claims: serde_json::Value = validate_and_decode_jwt(token)?;
/// request.extensions_mut().insert(JwtClaims::new(claims));
/// ```
///
/// The [`TenantLayer::with_claims`](crate::middleware::TenantLayer::with_claims)
/// method then extracts the tenant from these claims automatically.
#[derive(Debug, Clone)]
pub struct JwtClaims(Value);

impl JwtClaims {
    /// Create from decoded JWT claims (a JSON object).
    pub fn new(claims: Value) -> Self {
        Self(claims)
    }

    /// Create from a map of claim key-value pairs.
    pub fn from_map(claims: serde_json::Map<String, Value>) -> Self {
        Self(Value::Object(claims))
    }

    /// Get a claim value by name.
    pub fn get(&self, claim_name: &str) -> Option<&Value> {
        self.0.get(claim_name)
    }

    /// Get a claim as a string. Non-string values are converted via
    /// `to_string()`.
    pub fn get_str(&self, claim_name: &str) -> Option<String> {
        self.0.get(claim_name).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    /// Access the underlying JSON value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

/// Axum extractor for [`JwtClaims`] from request extensions.
///
/// Use this in handlers when you need access to the full decoded claims
/// beyond just the tenant ID.
///
/// ```rust,ignore
/// async fn handler(claims: JwtClaims) -> impl IntoResponse {
///     let org = claims.get_str("org_id").unwrap_or_default();
///     format!("Hello from org {org}")
/// }
/// ```
impl<S> FromRequestParts<S> for JwtClaims
where
    S: Send + Sync,
{
    type Rejection = crate::extractor::TenantRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<JwtClaims>()
            .cloned()
            .ok_or(crate::extractor::TenantRejection(
                TenantError::MissingTenant,
            ))
    }
}

/// Configuration for claims-based tenant resolution (re-exported for docs).
///
/// **You do not need to use this struct directly.** Instead, use
/// [`TenantLayer::with_claims`](crate::middleware::TenantLayer::with_claims):
///
/// ```rust,ignore
/// let layer = TenantLayer::new(fallback_resolver)
///     .with_claims("tenant_id");
/// ```
///
/// This struct exists for programmatic access to claim name configuration.
#[derive(Debug, Clone)]
pub struct ClaimsTenantResolver {
    claim_name: String,
}

impl ClaimsTenantResolver {
    /// Create a resolver that extracts the tenant from the given claim name.
    ///
    /// Common claim names: `"tenant_id"`, `"tenant"`, `"org_id"`, `"azp"`,
    /// `"realm"`.
    pub fn new(claim_name: impl Into<String>) -> Self {
        Self {
            claim_name: claim_name.into(),
        }
    }

    /// The claim name this resolver extracts.
    pub fn claim_name(&self) -> &str {
        &self.claim_name
    }

    /// Convert into a [`TenantLayer`](crate::middleware::TenantLayer) that
    /// uses claims-based resolution with the given fallback resolver.
    pub fn into_layer(
        self,
        fallback: impl tenant_core::resolver::TenantResolver,
    ) -> crate::middleware::TenantLayer {
        crate::middleware::TenantLayer::new(fallback).with_claims(self.claim_name)
    }
}

impl Default for ClaimsTenantResolver {
    fn default() -> Self {
        Self::new("tenant")
    }
}
