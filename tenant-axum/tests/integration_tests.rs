use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http::header;
use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
use tenant_axum::{
    CompositeTenantResolver, CookieTenantResolver, CurrentTenant, DefaultTenantResolver,
    HeaderTenantResolver, JwtTenantResolver, PathTenantResolver, QueryParamTenantResolver,
    SubdomainTenantResolver, TenantLayer,
};
use tower::ServiceExt;

fn test_app(layer: TenantLayer) -> Router {
    Router::new()
        .route("/hello", get(handler))
        .route("/{tenant}/resource", get(handler))
        .layer(layer)
}

async fn handler(CurrentTenant(tenant): CurrentTenant) -> String {
    format!("tenant:{}", tenant)
}

#[tokio::test]
async fn header_resolver_resolves_tenant() {
    let app = test_app(TenantLayer::new(HeaderTenantResolver::default()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "acme")
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
}

#[tokio::test]
async fn header_resolver_custom_header_name() {
    let resolver = HeaderTenantResolver::new("X-Custom-Tenant");
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Custom-Tenant", "corp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:corp");
}

#[tokio::test]
async fn missing_header_returns_400() {
    let app = test_app(TenantLayer::new(HeaderTenantResolver::default()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn cookie_resolver_resolves_tenant() {
    let resolver = CompositeTenantResolver::new().add(CookieTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("cookie", "tenant_cookie=acme; session=abc123")
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
}

#[tokio::test]
async fn cookie_resolver_custom_name() {
    let resolver = CompositeTenantResolver::new().add(CookieTenantResolver::new("my_tenant"));
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("cookie", "other=x; my_tenant=corp; session=y")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:corp");
}

#[tokio::test]
async fn cookie_resolver_missing_cookie_returns_400() {
    let resolver = CompositeTenantResolver::new().add(CookieTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("cookie", "session=abc123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cookie_resolver_no_cookie_header() {
    let resolver = CompositeTenantResolver::new().add(CookieTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
/// Build a minimal HS256-signed JWT with the given payload JSON.
/// Uses a fixed test secret key (signature verification is disabled by default
/// in JwtTenantResolver, so the key value doesn't matter for claim extraction).
fn make_jwt(payload_json: &str) -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let claims: serde_json::Value = serde_json::from_str(payload_json).unwrap();
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(b"test-secret"),
    )
    .unwrap()
}

#[tokio::test]
async fn jwt_resolver_resolves_tenant_from_claim() {
    let jwt = make_jwt(r#"{"sub":"user1","tenant":"acme","iat":1234567890}"#);
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
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
}

#[tokio::test]
async fn jwt_resolver_custom_claim_name() {
    let jwt = make_jwt(r#"{"sub":"user1","org_id":"corp","iat":1234567890}"#);
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::new("org_id"));
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:corp");
}

#[tokio::test]
async fn jwt_resolver_missing_claim_returns_400() {
    let jwt = make_jwt(r#"{"sub":"user1","iat":1234567890}"#);
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn jwt_resolver_no_auth_header_returns_400() {
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn jwt_resolver_non_bearer_scheme_returns_400() {
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn jwt_resolver_malformed_jwt_returns_400() {
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, "Bearer not-a-jwt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn path_resolver_resolves_first_segment() {
    let resolver = CompositeTenantResolver::new().add(PathTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/acme/resource")
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
}

#[tokio::test]
async fn path_resolver_custom_segment_index() {
    // /api/{tenant}/resource — segment 1
    let resolver = CompositeTenantResolver::new().add(PathTenantResolver::new(1));
    let app = Router::new()
        .route("/api/{tenant}/data", get(handler))
        .layer(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/corp/data")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:corp");
}
#[tokio::test]
async fn query_resolver_resolves_tenant() {
    let resolver = CompositeTenantResolver::new().add(QueryParamTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello?tenant_id=acme&other=val")
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
}

#[tokio::test]
async fn query_resolver_custom_param() {
    let resolver = CompositeTenantResolver::new().add(QueryParamTenantResolver::new("org"));
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello?org=corp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:corp");
}

#[tokio::test]
async fn query_resolver_missing_param_returns_400() {
    let resolver = CompositeTenantResolver::new().add(QueryParamTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello?other=val")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn subdomain_resolver_resolves_tenant() {
    let resolver = CompositeTenantResolver::new().add(SubdomainTenantResolver);
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "acme.example.com")
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
}

#[tokio::test]
async fn subdomain_resolver_strips_port() {
    let resolver = CompositeTenantResolver::new().add(SubdomainTenantResolver);
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "corp.example.com:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:corp");
}
#[tokio::test]
async fn default_resolver_provides_fallback() {
    let resolver = CompositeTenantResolver::new()
        .add(HeaderTenantResolver::default())
        .add(DefaultTenantResolver::new("public").unwrap());
    let app = test_app(TenantLayer::new(resolver));

    // No header → falls through to default
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
async fn default_resolver_not_used_when_header_present() {
    let resolver = CompositeTenantResolver::new()
        .add(HeaderTenantResolver::default())
        .add(DefaultTenantResolver::new("public").unwrap());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "acme")
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
}

#[tokio::test]
async fn composite_header_then_cookie_uses_header_first() {
    let resolver = CompositeTenantResolver::new()
        .add(HeaderTenantResolver::default())
        .add(CookieTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    // Both header and cookie present → header wins
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "from-header")
                .header("cookie", "tenant_cookie=from-cookie")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:from-header");
}

#[tokio::test]
async fn composite_falls_through_to_cookie_when_no_header() {
    let resolver = CompositeTenantResolver::new()
        .add(HeaderTenantResolver::default())
        .add(CookieTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("cookie", "tenant_cookie=from-cookie")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:from-cookie");
}
#[tokio::test]
async fn config_driven_header_strategy() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Header)
        .header_name("X-Org")
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
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
}

#[tokio::test]
async fn config_driven_multi_strategy_with_fallback() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Header)
        .strategy(HttpTenantStrategy::Cookie)
        .default_tenant("public")
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    // 1) Header present → resolves from header
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "from-header")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:from-header");

    // 2) No header, cookie present → resolves from cookie
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("cookie", "tenant_cookie=from-cookie")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:from-cookie");

    // 3) Nothing → falls back to default
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:public");
}

#[tokio::test]
async fn config_driven_jwt_strategy() {
    let jwt = make_jwt(r#"{"sub":"user1","tid":"acme-corp"}"#);
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Jwt)
        .jwt_claim_name("tid")
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:acme-corp");
}

#[tokio::test]
async fn config_driven_cookie_strategy() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Cookie)
        .cookie_name("org_cookie")
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("cookie", "org_cookie=acme")
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
}

#[tokio::test]
async fn config_driven_query_strategy() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Query)
        .query_param_name("org")
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello?org=acme")
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
}

#[tokio::test]
async fn config_driven_path_strategy() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Path)
        .path_segment_index(0)
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/acme/resource")
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
}

#[tokio::test]
async fn config_driven_subdomain_strategy() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Subdomain)
        .build();

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "acme.example.com")
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
}

#[tokio::test]
async fn config_default_returns_header_strategy() {
    let config = HttpTenantConfig::default();
    assert_eq!(config.strategies, vec![HttpTenantStrategy::Header]);
    assert_eq!(config.header_name, "X-Tenant-Id");

    let app = test_app(TenantLayer::from_config(&config));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "acme")
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
}
#[test]
fn config_builder_defaults() {
    let config = HttpTenantConfig::default();
    assert!(config.enabled);
    assert_eq!(config.strategies, vec![HttpTenantStrategy::Header]);
    assert_eq!(config.header_name, "X-Tenant-Id");
    assert_eq!(config.jwt_claim_name, "tenant");
    assert_eq!(config.cookie_name, "tenant_cookie");
    assert_eq!(config.path_segment_index, 0);
    assert_eq!(config.query_param_name, "tenant_id");
    assert!(config.default_tenant.is_none());
}

#[test]
fn config_builder_all_fields() {
    let config = HttpTenantConfig::builder()
        .enabled(false)
        .strategy(HttpTenantStrategy::Jwt)
        .strategy(HttpTenantStrategy::Cookie)
        .header_name("X-Org")
        .jwt_claim_name("org_id")
        .cookie_name("org_cookie")
        .path_segment_index(2)
        .query_param_name("org")
        .default_tenant("default-org")
        .build();

    assert!(!config.enabled);
    assert_eq!(
        config.strategies,
        vec![HttpTenantStrategy::Jwt, HttpTenantStrategy::Cookie]
    );
    assert_eq!(config.header_name, "X-Org");
    assert_eq!(config.jwt_claim_name, "org_id");
    assert_eq!(config.cookie_name, "org_cookie");
    assert_eq!(config.path_segment_index, 2);
    assert_eq!(config.query_param_name, "org");
    assert_eq!(config.default_tenant.as_deref(), Some("default-org"));
}

#[test]
fn config_serde_roundtrip() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Header)
        .strategy(HttpTenantStrategy::Jwt)
        .default_tenant("public")
        .build();

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: HttpTenantConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.strategies, config.strategies);
    assert_eq!(deserialized.default_tenant, config.default_tenant);
    assert_eq!(deserialized.header_name, config.header_name);
}

#[test]
fn config_strategies_replace() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Header)
        .strategies(vec![HttpTenantStrategy::Cookie, HttpTenantStrategy::Jwt])
        .build();

    assert_eq!(
        config.strategies,
        vec![HttpTenantStrategy::Cookie, HttpTenantStrategy::Jwt]
    );
}
#[tokio::test]
async fn rejection_missing_tenant_is_400() {
    let app = test_app(TenantLayer::new(HeaderTenantResolver::default()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&body)
        .unwrap()
        .contains("No tenant identifier"));
}
#[tokio::test]
async fn different_tenants_isolated() {
    let config = HttpTenantConfig::builder()
        .strategy(HttpTenantStrategy::Header)
        .build();
    let app = test_app(TenantLayer::from_config(&config));

    // Tenant A
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "tenant-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:tenant-a");

    // Tenant B — same app, different tenant
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "tenant-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:tenant-b");
}

// ─── Subdomain resolver edge cases ───────────────────────────────────

#[tokio::test]
async fn subdomain_resolver_rejects_bare_domain() {
    // "example.com" has only 2 segments — should NOT resolve
    let resolver = CompositeTenantResolver::new().add(SubdomainTenantResolver);
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn subdomain_resolver_rejects_localhost() {
    let resolver = CompositeTenantResolver::new().add(SubdomainTenantResolver);
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn subdomain_resolver_rejects_localhost_with_port() {
    let resolver = CompositeTenantResolver::new().add(SubdomainTenantResolver);
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn subdomain_resolver_resolves_deep_subdomain() {
    // "tenant.us-east.example.com" — 4 segments, first is tenant
    let resolver = CompositeTenantResolver::new().add(SubdomainTenantResolver);
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("host", "acme.us-east.example.com")
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
}

// ─── TenantId trimming in resolvers ──────────────────────────────────

#[tokio::test]
async fn header_resolver_trims_whitespace() {
    let app = test_app(TenantLayer::new(HeaderTenantResolver::default()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "  acme  ")
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
}

#[tokio::test]
async fn header_resolver_rejects_whitespace_only_value() {
    let app = test_app(TenantLayer::new(HeaderTenantResolver::default()));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-Id", "   ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ─── JWT with numeric claim ──────────────────────────────────────────

#[tokio::test]
async fn jwt_resolver_numeric_claim_value() {
    let jwt = make_jwt(r#"{"sub":"user1","tenant":12345,"iat":1234567890}"#);
    let resolver = CompositeTenantResolver::new().add(JwtTenantResolver::default());
    let app = test_app(TenantLayer::new(resolver));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header(header::AUTHORIZATION, format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:12345");
}

// ─── Claims-based resolution tests ──────────────────────────────────────────

#[tokio::test]
async fn claims_based_resolution_from_extensions() {
    use tenant_axum::claims::JwtClaims;

    let layer = TenantLayer::new(HeaderTenantResolver::default()).with_claims("tenant_id");
    let app = test_app(layer);

    // Simulate an auth middleware inserting JwtClaims into extensions
    let claims = serde_json::json!({ "tenant_id": "acme", "sub": "user1" });
    let mut req = Request::builder()
        .uri("/hello")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(JwtClaims::new(claims));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:acme");
}

#[tokio::test]
async fn claims_based_resolution_falls_back_to_resolver_chain() {
    let layer = TenantLayer::new(HeaderTenantResolver::default()).with_claims("tenant_id");
    let app = test_app(layer);

    // No JwtClaims in extensions — should fall back to header resolver
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-ID", "fallback-tenant")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:fallback-tenant");
}

#[tokio::test]
async fn claims_based_resolution_missing_claim_falls_back() {
    use tenant_axum::claims::JwtClaims;

    let layer = TenantLayer::new(HeaderTenantResolver::default()).with_claims("tenant_id");
    let app = test_app(layer);

    // JwtClaims present but the claim name is missing — fallback to header
    let claims = serde_json::json!({ "sub": "user1", "org": "acme" });
    let mut req = Request::builder()
        .uri("/hello")
        .header("X-Tenant-ID", "header-tenant")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(JwtClaims::new(claims));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:header-tenant");
}

#[tokio::test]
async fn claims_based_resolution_numeric_claim() {
    use tenant_axum::claims::JwtClaims;

    let layer = TenantLayer::new(HeaderTenantResolver::default()).with_claims("org_id");
    let app = test_app(layer);

    // Numeric claim should be converted to string
    let claims = serde_json::json!({ "org_id": 42, "sub": "user1" });
    let mut req = Request::builder()
        .uri("/hello")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(JwtClaims::new(claims));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "tenant:42");
}

#[tokio::test]
async fn claims_based_resolution_without_with_claims_ignores_extensions() {
    use tenant_axum::claims::JwtClaims;

    // No .with_claims() — extensions should be ignored
    let layer = TenantLayer::new(HeaderTenantResolver::default());
    let app = test_app(layer);

    let claims = serde_json::json!({ "tenant_id": "claims-tenant" });
    let mut req = Request::builder()
        .uri("/hello")
        .header("X-Tenant-ID", "header-tenant")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(JwtClaims::new(claims));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    // Should resolve from header, NOT from claims
    assert_eq!(body, "tenant:header-tenant");
}

#[tokio::test]
async fn claims_based_resolution_no_claims_no_fallback_returns_400() {
    let layer = TenantLayer::new(HeaderTenantResolver::default()).with_claims("tenant_id");
    let app = test_app(layer);

    // No JwtClaims, no header → 400
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ─── Skip paths tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn skip_paths_bypasses_tenant_resolution() {
    let layer = TenantLayer::new(HeaderTenantResolver::default())
        .with_skip_paths(vec!["/health", "/ready"]);

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(|| async { "ok" }))
        .route("/api", get(handler))
        .layer(layer);

    // /health should pass without tenant header
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body, "ok");

    // /ready should also pass without tenant header
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // /api without tenant should still fail
    let resp = app
        .oneshot(Request::builder().uri("/api").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn skip_paths_prefix_matching() {
    let layer = TenantLayer::new(HeaderTenantResolver::default()).with_skip_paths(vec!["/public/"]);

    let app = Router::new()
        .route("/public/docs", get(|| async { "docs" }))
        .route("/api", get(handler))
        .layer(layer);

    // /public/docs should match prefix "/public/"
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/public/docs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // /api still requires tenant
    let resp = app
        .oneshot(Request::builder().uri("/api").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ─── Metrics layer tests ────────────────────────────────────────────────────

#[tokio::test]
async fn metrics_layer_tracks_resolution_outcomes() {
    use tenant_axum::metrics::MetricsTenantLayer;

    let tenant_layer = TenantLayer::new(HeaderTenantResolver::default());
    let metrics_layer = MetricsTenantLayer::new(tenant_layer);
    let metrics = metrics_layer.metrics();

    let app = Router::new()
        .route("/hello", get(handler))
        .layer(metrics_layer);

    // Successful resolution
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .header("X-Tenant-ID", "acme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Failed resolution (no header)
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let snap = metrics.snapshot();
    assert_eq!(snap.requests_total, 2);
    assert_eq!(snap.resolved_total, 1);
    assert_eq!(snap.missing_total, 1);
    assert_eq!(snap.errors_total, 0);
}

// ─── AsyncTenantResolver tests ──────────────────────────────────────────────

#[tokio::test]
async fn async_resolver_blanket_impl_works() {
    use tenant_core::resolver::{AsyncTenantResolver, ResolutionContext};

    struct SimpleCtx;
    impl ResolutionContext for SimpleCtx {
        fn header(&self, name: &str) -> Option<&str> {
            if name == "X-Tenant-Id" {
                Some("async-tenant")
            } else {
                None
            }
        }
    }

    let resolver = HeaderTenantResolver::default();
    // Use the AsyncTenantResolver blanket impl
    let result = AsyncTenantResolver::resolve(&resolver, &SimpleCtx).await;
    assert!(result.is_ok());
    let tenant = result.unwrap();
    assert!(tenant.is_some());
    assert_eq!(tenant.unwrap().as_str(), "async-tenant");
}
