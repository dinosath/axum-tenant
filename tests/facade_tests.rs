use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::ServiceExt;

/// Tests that the top-level `axum-tenant` crate correctly re-exports
/// everything from tenant-core, tenant-axum, and tenant-sea-orm through
/// the feature-gated facade modules.
async fn handler(
    axum_tenant::axum::CurrentTenant(tenant): axum_tenant::axum::CurrentTenant,
) -> String {
    format!("tenant:{}", tenant)
}

#[tokio::test]
async fn facade_reexports_core_types() {
    // Verify core types are available at the top level
    let id = axum_tenant::TenantId::new("acme").unwrap();
    assert_eq!(id.as_str(), "acme");

    let config = axum_tenant::TenantConfig::builder()
        .strategy(axum_tenant::MultiTenancyStrategy::Database)
        .build();
    assert_eq!(config.strategy, axum_tenant::MultiTenancyStrategy::Database);
}

#[tokio::test]
async fn facade_reexports_axum_module() {
    use axum_tenant::axum::*;

    let config = config::HttpTenantConfig::builder()
        .strategy(config::HttpTenantStrategy::Header)
        .header_name("X-Org")
        .default_tenant("public")
        .build();

    let app = Router::new()
        .route("/hello", get(handler))
        .layer(TenantLayer::from_config(&config));

    // Test with header
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Org", "acme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:acme");

    // Test fallback
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:public");
}

#[tokio::test]
#[cfg(any(feature = "sea-orm", feature = "sea-orm-2"))]
async fn facade_reexports_orm_module() {
    // Verify SeaORM types are accessible through the facade
    use axum_tenant::orm::TenantFilter;
    // TenantFilter is a unit struct, just verify it's importable
    let _ = TenantFilter;
}
