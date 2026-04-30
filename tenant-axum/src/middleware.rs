use crate::claims::JwtClaims;
use crate::context::HttpResolutionContext;
use crate::extractor::{TenantContext, TenantRejection};
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tenant_core::error::TenantError;
use tenant_core::resolver::TenantResolver;
use tenant_core::tenant::{MultiTenancyStrategy, TenantId};
use tower_layer::Layer;
use tower_service::Service;

/// Tower layer that resolves the tenant from each incoming request and
/// injects the [`TenantId`](tenant_core::TenantId) into request extensions.
///
/// Implemented as a Tower middleware for composability with the Axum
/// ecosystem.
///
/// # Claims-based resolution
///
/// If an upstream auth middleware inserts [`JwtClaims`] into request
/// extensions, the `TenantLayer` can resolve the tenant directly from those
/// pre-validated claims — no JWT parsing needed. Use
/// [`TenantLayer::with_claims`] to configure this.
///
/// # Example
///
/// ```rust,ignore
/// use tenant_axum::{TenantLayer, HeaderTenantResolver, CompositeTenantResolver};
///
/// let resolver = CompositeTenantResolver::new()
///     .add(HeaderTenantResolver::default());
///
/// let app = Router::new()
///     .route("/api", get(handler))
///     .layer(TenantLayer::new(resolver));
/// ```
#[derive(Clone)]
pub struct TenantLayer {
    resolver: Arc<dyn TenantResolver>,
    /// If set, the middleware will first check for [`JwtClaims`] in request
    /// extensions and extract the tenant from this claim name.
    claims_claim_name: Option<String>,
    /// Paths that bypass tenant resolution entirely.
    skip_paths: Arc<Vec<String>>,
    /// The multi-tenancy strategy, propagated into `TenantContext`.
    strategy: Option<MultiTenancyStrategy>,
}

impl TenantLayer {
    pub fn new(resolver: impl TenantResolver) -> Self {
        Self {
            resolver: Arc::new(resolver),
            claims_claim_name: None,
            skip_paths: Arc::new(Vec::new()),
            strategy: None,
        }
    }

    /// Enable claims-based tenant resolution as the **primary** resolution
    /// strategy.
    ///
    /// When enabled, the middleware first checks if [`JwtClaims`] exist in
    /// request extensions (placed there by an upstream auth middleware). If
    /// found, the tenant is extracted from the specified claim. If not found
    /// or the claim is absent, the standard resolver chain is used as
    /// fallback.
    ///
    /// This is the **recommended** approach for JWT-based tenant resolution:
    ///
    /// ```rust,ignore
    /// use tenant_axum::{TenantLayer, HeaderTenantResolver};
    ///
    /// let layer = TenantLayer::new(HeaderTenantResolver::default())
    ///     .with_claims("tenant_id");
    ///
    /// // Upstream auth middleware inserts JwtClaims → tenant resolved from claims
    /// // No auth middleware / no JwtClaims → falls back to HeaderTenantResolver
    /// ```
    pub fn with_claims(mut self, claim_name: impl Into<String>) -> Self {
        self.claims_claim_name = Some(claim_name.into());
        self
    }

    /// Skip tenant resolution for requests matching the given path prefixes.
    ///
    /// Requests whose URI path starts with any of the provided prefixes will
    /// pass through without tenant resolution (no `TenantId` in extensions).
    /// Useful for health checks, readiness probes, and public endpoints.
    ///
    /// ```rust,ignore
    /// let layer = TenantLayer::new(HeaderTenantResolver::default())
    ///     .with_skip_paths(vec!["/health", "/ready", "/metrics"]);
    /// ```
    pub fn with_skip_paths(mut self, paths: Vec<impl Into<String>>) -> Self {
        self.skip_paths = Arc::new(paths.into_iter().map(Into::into).collect());
        self
    }

    /// Set the multi-tenancy strategy that will be propagated into
    /// [`TenantContext`](crate::extractor::TenantContext) for downstream
    /// handlers.
    pub fn with_strategy(mut self, strategy: MultiTenancyStrategy) -> Self {
        self.strategy = Some(strategy);
        self
    }

    /// Create a `TenantLayer` from an [`HttpTenantConfig`](crate::config::HttpTenantConfig).
    ///
    /// If the config includes the `Jwt` strategy, claims-based resolution is
    /// automatically enabled using
    /// [`jwt_claim_name`](crate::config::HttpTenantConfig::jwt_claim_name).
    /// An upstream auth middleware is expected to validate the JWT and insert
    /// [`JwtClaims`] into request extensions; the tenant is then resolved
    /// from the configured claim.
    ///
    /// If the config has `enabled = false`, the returned layer will still
    /// exist but pass requests through without tenant resolution. Check
    /// [`HttpTenantConfig::enabled`](crate::config::HttpTenantConfig::enabled)
    /// and conditionally apply the layer if you prefer.
    pub fn from_config(config: &crate::config::HttpTenantConfig) -> Self {
        use crate::config::HttpTenantStrategy;

        let claims_claim_name = if config.strategies.contains(&HttpTenantStrategy::Jwt) {
            Some(config.jwt_claim_name.clone())
        } else {
            None
        };

        Self {
            resolver: Arc::new(config.into_resolver()),
            claims_claim_name,
            skip_paths: Arc::new(Vec::new()),
            strategy: None,
        }
    }
}

impl<S> Layer<S> for TenantLayer {
    type Service = TenantMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TenantMiddleware {
            inner,
            resolver: Arc::clone(&self.resolver),
            claims_claim_name: self.claims_claim_name.clone(),
            skip_paths: Arc::clone(&self.skip_paths),
            strategy: self.strategy,
        }
    }
}

/// The middleware service created by [`TenantLayer`].
#[derive(Clone)]
pub struct TenantMiddleware<S> {
    inner: S,
    resolver: Arc<dyn TenantResolver>,
    claims_claim_name: Option<String>,
    skip_paths: Arc<Vec<String>>,
    strategy: Option<MultiTenancyStrategy>,
}

impl<S> Service<Request> for TenantMiddleware<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let resolver = Arc::clone(&self.resolver);
        let claims_claim_name = self.claims_claim_name.clone();
        let skip_paths = Arc::clone(&self.skip_paths);
        let strategy = self.strategy;
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Check if this path should skip tenant resolution
            let path = req.uri().path();
            if skip_paths
                .iter()
                .any(|prefix| path.starts_with(prefix.as_str()))
            {
                return inner.call(req).await;
            }

            let (mut parts, body) = req.into_parts();

            // 1. Try claims-based resolution first (if configured)
            let tenant_from_claims = if let Some(ref claim_name) = claims_claim_name {
                parts
                    .extensions
                    .get::<JwtClaims>()
                    .and_then(|claims| claims.get_str(claim_name))
                    .and_then(TenantId::new)
            } else {
                None
            };

            let (result, resolved_by) = if let Some(tenant_id) = tenant_from_claims {
                // Resolved from pre-validated claims — no parsing needed
                tracing::debug!(tenant = %tenant_id, "Tenant resolved from JWT claims");
                (Ok(Some(tenant_id)), "JwtClaims")
            } else {
                // 2. Fall back to resolver chain
                let ctx = HttpResolutionContext::new(&parts);
                let name = resolver.name();
                (resolver.resolve(&ctx), name)
            };

            match result {
                Ok(Some(tenant_id)) => {
                    tracing::debug!(tenant = %tenant_id, "Tenant resolved");
                    let tenant_ctx = TenantContext::new(tenant_id, resolved_by, strategy);
                    parts.extensions.insert(tenant_ctx);
                    let req = Request::from_parts(parts, body);
                    inner.call(req).await
                }
                Ok(None) => {
                    tracing::warn!("No resolver could determine the tenant");
                    Ok(TenantRejection(TenantError::MissingTenant).into_response())
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Tenant resolution failed");
                    Ok(TenantRejection(e).into_response())
                }
            }
        })
    }
}
