//! Middleware that auto-injects tenant-scoped `DatabaseConnection` into
//! request extensions.
//!
//! Place this layer **after** the `TenantLayer` (which inserts `TenantId`).
//! It reads the resolved `TenantId` from extensions, calls the
//! [`TenantConnectionProvider`] to get the connection, and inserts the
//! `DatabaseConnection` into extensions for downstream handlers to extract.
//!
//! # Example
//!
//! ```rust,ignore
//! use tenant_axum::TenantLayer;
//! use tenant_sea_orm::middleware::TenantDbLayer;
//!
//! let app = Router::new()
//!     .route("/api", get(handler))
//!     .layer(TenantDbLayer::new(db_provider))
//!     .layer(TenantLayer::new(resolver)); // runs first
//!
//! async fn handler(
//!     tenant_db: TenantDb,  // auto-extracted DatabaseConnection
//! ) -> impl IntoResponse {
//!     let items = Entity::find().all(&*tenant_db).await?;
//!     // ...
//! }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use sea_orm::DatabaseConnection;
use tenant_core::tenant::TenantId;
use tower_layer::Layer;
use tower_service::Service;

use crate::connection::TenantConnectionProvider;

/// Newtype wrapper for tenant-scoped `DatabaseConnection` in extensions.
///
/// Extract this in handlers to get the connection for the current tenant.
#[derive(Debug, Clone)]
pub struct TenantDb(pub DatabaseConnection);

impl std::ops::Deref for TenantDb {
    type Target = DatabaseConnection;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Tower layer that resolves the tenant's `DatabaseConnection` and injects
/// it into request extensions as [`TenantDb`].
///
/// Must be placed **after** `TenantLayer` in the middleware stack (i.e.,
/// applied first to the router, so it runs after `TenantLayer` resolves
/// the `TenantId`).
#[derive(Clone)]
pub struct TenantDbLayer<P: TenantConnectionProvider> {
    provider: Arc<P>,
}

impl<P: TenantConnectionProvider> TenantDbLayer<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider: Arc::new(provider),
        }
    }
}

impl<S, P: TenantConnectionProvider> Layer<S> for TenantDbLayer<P> {
    type Service = TenantDbMiddleware<S, P>;

    fn layer(&self, inner: S) -> Self::Service {
        TenantDbMiddleware {
            inner,
            provider: Arc::clone(&self.provider),
        }
    }
}

/// The middleware service created by [`TenantDbLayer`].
#[derive(Clone)]
pub struct TenantDbMiddleware<S, P: TenantConnectionProvider> {
    inner: S,
    provider: Arc<P>,
}

impl<S, P> Service<Request> for TenantDbMiddleware<S, P>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    P: TenantConnectionProvider,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let provider = Arc::clone(&self.provider);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let (mut parts, body) = req.into_parts();

            // Get the TenantId from extensions (set by TenantLayer)
            let tenant_id = parts.extensions.get::<TenantId>().cloned();

            if let Some(ref tenant) = tenant_id {
                match provider.connection_for(tenant).await {
                    Ok(conn) => {
                        parts.extensions.insert(TenantDb(conn));
                    }
                    Err(e) => {
                        let response = (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to get tenant database connection: {e}"),
                        )
                            .into_response();
                        return Ok(response);
                    }
                }
            }

            let req = Request::from_parts(parts, body);
            inner.call(req).await
        })
    }
}
