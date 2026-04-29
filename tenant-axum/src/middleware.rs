use crate::context::HttpResolutionContext;
use crate::extractor::TenantRejection;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tenant_core::error::TenantError;
use tenant_core::resolver::TenantResolver;
use tower_layer::Layer;
use tower_service::Service;

/// Tower layer that resolves the tenant from each incoming request and
/// injects the [`TenantId`](tenant_core::TenantId) into request extensions.
///
/// Implemented as a Tower middleware for composability with the Axum
/// ecosystem.
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
}

impl TenantLayer {
    pub fn new(resolver: impl TenantResolver) -> Self {
        Self {
            resolver: Arc::new(resolver),
        }
    }

    /// Create a `TenantLayer` from an [`HttpTenantConfig`](crate::config::HttpTenantConfig).
    ///
    /// If the config has `enabled = false`, the returned layer will still
    /// exist but pass requests through without tenant resolution. Check
    /// [`HttpTenantConfig::enabled`](crate::config::HttpTenantConfig::enabled)
    /// and conditionally apply the layer if you prefer.
    ///
    /// ```rust,ignore
    /// use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
    /// use tenant_axum::TenantLayer;
    ///
    /// let config = HttpTenantConfig::builder()
    ///     .strategy(HttpTenantStrategy::Header)
    ///     .strategy(HttpTenantStrategy::Jwt)
    ///     .default_tenant("public")
    ///     .build();
    ///
    /// let app = Router::new()
    ///     .route("/api", get(handler))
    ///     .layer(TenantLayer::from_config(&config));
    /// ```
    pub fn from_config(config: &crate::config::HttpTenantConfig) -> Self {
        Self::new(config.into_resolver())
    }
}

impl<S> Layer<S> for TenantLayer {
    type Service = TenantMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TenantMiddleware {
            inner,
            resolver: Arc::clone(&self.resolver),
        }
    }
}

/// The middleware service created by [`TenantLayer`].
#[derive(Clone)]
pub struct TenantMiddleware<S> {
    inner: S,
    resolver: Arc<dyn TenantResolver>,
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
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let (mut parts, body) = req.into_parts();
            let ctx = HttpResolutionContext::new(&parts);

            match resolver.resolve(&ctx) {
                Ok(Some(tenant_id)) => {
                    tracing::debug!(tenant = %tenant_id, "Tenant resolved");
                    parts.extensions.insert(tenant_id);
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
