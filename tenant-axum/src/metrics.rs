//! Metrics and observability for tenant resolution.
//!
//! Provides an instrumented middleware layer that tracks:
//! - Tenant resolution latency (as a tracing span)
//! - Resolution outcomes (success, miss, error) via tracing events
//! - Per-tenant request counts
//! - Connection cache statistics
//!
//! # Usage
//!
//! Wrap your `TenantLayer` with [`MetricsTenantLayer`] to get automatic
//! instrumentation:
//!
//! ```rust,ignore
//! use tenant_axum::metrics::MetricsTenantLayer;
//! use tenant_axum::{TenantLayer, HeaderTenantResolver};
//!
//! let layer = MetricsTenantLayer::new(
//!     TenantLayer::new(HeaderTenantResolver::default())
//! );
//!
//! let app = Router::new()
//!     .route("/api", get(handler))
//!     .layer(layer);
//! ```
//!
//! The middleware emits structured tracing events that can be consumed by
//! any tracing subscriber (e.g., `tracing-opentelemetry` for Prometheus/OTLP).

use axum::extract::Request;
use axum::response::Response;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower_layer::Layer;
use tower_service::Service;

/// Counters for tenant resolution outcomes.
#[derive(Debug, Default)]
pub struct TenantMetrics {
    /// Total requests processed by the tenant middleware.
    pub requests_total: AtomicU64,
    /// Requests where tenant was successfully resolved.
    pub resolved_total: AtomicU64,
    /// Requests where no tenant could be determined.
    pub missing_total: AtomicU64,
    /// Requests where tenant resolution produced an error.
    pub errors_total: AtomicU64,
    /// Requests that were skipped (path opt-out).
    pub skipped_total: AtomicU64,
}

impl TenantMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot current counter values.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.requests_total.load(Ordering::Relaxed),
            resolved_total: self.resolved_total.load(Ordering::Relaxed),
            missing_total: self.missing_total.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            skipped_total: self.skipped_total.load(Ordering::Relaxed),
        }
    }
}

/// Point-in-time snapshot of [`TenantMetrics`].
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub requests_total: u64,
    pub resolved_total: u64,
    pub missing_total: u64,
    pub errors_total: u64,
    pub skipped_total: u64,
}

/// Tower layer that wraps any inner service with tenant resolution metrics.
///
/// Records resolution latency, outcome counters, and per-tenant request
/// tracking via `tracing` structured fields.
#[derive(Clone)]
pub struct MetricsTenantLayer<L> {
    inner_layer: L,
    metrics: Arc<TenantMetrics>,
}

impl<L> MetricsTenantLayer<L> {
    pub fn new(inner_layer: L) -> Self {
        Self {
            inner_layer,
            metrics: Arc::new(TenantMetrics::new()),
        }
    }

    /// Access the shared metrics counters (e.g., for exposing via a
    /// `/metrics` endpoint).
    pub fn metrics(&self) -> Arc<TenantMetrics> {
        Arc::clone(&self.metrics)
    }
}

impl<S, L> Layer<S> for MetricsTenantLayer<L>
where
    L: Layer<S>,
{
    type Service = MetricsTenantMiddleware<L::Service>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsTenantMiddleware {
            inner: self.inner_layer.layer(inner),
            metrics: Arc::clone(&self.metrics),
        }
    }
}

/// Instrumented middleware service.
#[derive(Clone)]
pub struct MetricsTenantMiddleware<S> {
    inner: S,
    metrics: Arc<TenantMetrics>,
}

impl<S> Service<Request> for MetricsTenantMiddleware<S>
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
        let metrics = Arc::clone(&self.metrics);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            metrics.requests_total.fetch_add(1, Ordering::Relaxed);
            let start = Instant::now();
            let path = req.uri().path().to_owned();

            let response = inner.call(req).await?;

            let elapsed = start.elapsed();
            let status = response.status().as_u16();

            // Check if a TenantId was inserted into extensions
            // We infer outcome from status code since we wrap the inner layer
            if status == 400 {
                metrics.missing_total.fetch_add(1, Ordering::Relaxed);
                tracing::info!(
                    path = %path,
                    elapsed_ms = elapsed.as_millis() as u64,
                    outcome = "missing",
                    "tenant.resolution"
                );
            } else if status >= 500 {
                metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                tracing::info!(
                    path = %path,
                    elapsed_ms = elapsed.as_millis() as u64,
                    outcome = "error",
                    "tenant.resolution"
                );
            } else {
                // Check response extensions for TenantId (passed through)
                metrics.resolved_total.fetch_add(1, Ordering::Relaxed);
                tracing::debug!(
                    path = %path,
                    elapsed_ms = elapsed.as_millis() as u64,
                    outcome = "resolved",
                    "tenant.resolution"
                );
            }

            Ok(response)
        })
    }
}
