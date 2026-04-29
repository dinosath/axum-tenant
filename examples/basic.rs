//! Basic example: config-driven tenant resolution with Axum.
//!
//! ```bash
//! cargo run --example basic --features axum
//! curl -H "X-Tenant-Id: acme" http://localhost:3000/hello
//! ```

use axum::{routing::get, Router};
use tenant_axum::{
    config::{HttpTenantConfig, HttpTenantStrategy},
    CurrentTenant, TenantLayer,
};
use tracing_subscriber;

async fn hello(CurrentTenant(tenant): CurrentTenant) -> String {
    format!("Hello from tenant: {tenant}")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Configure tenant resolution via HttpTenantConfig builder.
    let config = HttpTenantConfig::builder()
        .enabled(true)
        .strategy(HttpTenantStrategy::Header)
        .strategy(HttpTenantStrategy::Cookie)
        .header_name("X-Tenant-Id")
        .cookie_name("tenant_cookie")
        .default_tenant("public")
        .build();

    let app: Router = Router::new()
        .route("/hello", get(hello))
        .layer(TenantLayer::from_config(&config));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
