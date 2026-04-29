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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
/// Build a minimal JWT with the given payload JSON (no signature verification).
fn make_jwt(payload_json: &str) -> String {
    fn base64url_encode(data: &[u8]) -> String {
        // Minimal base64url encoder
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = Vec::new();
        let mut bits: u32 = 0;
        let mut nbits: u32 = 0;
        for &byte in data {
            bits = (bits << 8) | byte as u32;
            nbits += 8;
            while nbits >= 6 {
                nbits -= 6;
                result.push(TABLE[((bits >> nbits) & 0x3f) as usize]);
            }
        }
        if nbits > 0 {
            bits <<= 6 - nbits;
            result.push(TABLE[(bits & 0x3f) as usize]);
        }
        let mut s = String::from_utf8(result).unwrap();
        // Convert to URL-safe
        s = s.replace('+', "-").replace('/', "_");
        // Strip padding
        s.trim_end_matches('=').to_string()
    }

    let header = base64url_encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
    let payload = base64url_encode(payload_json.as_bytes());
    let signature = base64url_encode(b"fakesig");
    format!("{}.{}.{}", header, payload, signature)
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("No tenant identifier")
    );
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "tenant:tenant-b");
}
