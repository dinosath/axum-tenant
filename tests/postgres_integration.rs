//! End-to-end integration tests with a real PostgreSQL container and Axum
//! server.
//!
//! Tests exercise the **full request→middleware→handler→SeaORM→Postgres**
//! pipeline for all three multi-tenancy strategies:
//!
//! - **Schema-per-tenant** — `SET search_path` isolation
//! - **Discriminator** — shared table with `tenant_id` column
//! - **Database-per-tenant** — separate databases per tenant
//!
//! Requires Docker to be running.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use sea_orm::{
    ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend,
    EntityTrait, Schema, Set, Statement,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
use tenant_axum::{CurrentTenant, TenantLayer};
use tenant_core::TenantId;
use tenant_sea_orm::{
    DatabasePerTenantProvider, DiscriminatorProvider, SchemaPerTenantProvider, TenantAware,
    TenantConnectionProvider, TenantDatabaseMapping, TenantFilter, TenantSchemaMapping,
};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::net::TcpListener;

mod product {
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "products")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub name: String,
        pub tenant_id: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

impl TenantAware for product::Entity {
    fn tenant_column() -> product::Column {
        product::Column::TenantId
    }
}

async fn start_postgres() -> (ContainerAsync<Postgres>, String) {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        host_port
    );
    (container, url)
}

async fn connect(url: &str) -> DatabaseConnection {
    Database::connect(url).await.unwrap()
}

/// Connect with a pool size of 1 so session-level SET commands stick.
async fn connect_pool1(url: &str) -> DatabaseConnection {
    let mut opts = ConnectOptions::new(url);
    opts.max_connections(1).min_connections(1);
    Database::connect(opts).await.unwrap()
}

/// Create the `products` table in the given schema (or `public` by default).
async fn create_products_table(db: &DatabaseConnection) {
    let backend = db.get_database_backend();
    let schema = Schema::new(backend);
    let stmt = schema.create_table_from_entity(product::Entity);
    db.execute(backend.build(&stmt)).await.unwrap();
}

/// Seed products for a specific tenant.
async fn seed_products(db: &DatabaseConnection, tenant: &str, names: &[&str]) {
    for name in names {
        product::ActiveModel {
            name: Set(ToString::to_string(name)),
            tenant_id: Set(tenant.to_string()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }
}

/// Find a free TCP port on localhost.
async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// Start an Axum app in the background and return its base URL.
async fn serve(app: Router) -> String {
    let port = free_port().await;
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let listener = TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give the server a moment to bind
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://127.0.0.1:{}", port)
}

#[derive(Debug, Serialize, Deserialize)]
struct ProductList {
    tenant: String,
    products: Vec<String>,
}

#[tokio::test]
async fn discriminator_strategy_isolates_tenants() {
    let (_container, url) = start_postgres().await;
    let db = connect(&url).await;
    create_products_table(&db).await;

    seed_products(&db, "acme", &["Widget", "Gadget"]).await;
    seed_products(&db, "globex", &["Sprocket", "Cog", "Gear"]).await;

    let provider = Arc::new(DiscriminatorProvider::new(db));

    let app = Router::new()
        .route("/products", get(discriminator_list_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "acme")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "acme");
    assert_eq!(resp.products.len(), 2);
    assert!(resp.products.contains(&"Widget".to_string()));
    assert!(resp.products.contains(&"Gadget".to_string()));

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "globex")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "globex");
    assert_eq!(resp.products.len(), 3);
    assert!(resp.products.contains(&"Sprocket".to_string()));
}

async fn discriminator_list_handler(
    CurrentTenant(tenant): CurrentTenant,
    State(provider): State<Arc<DiscriminatorProvider>>,
) -> axum::Json<ProductList> {
    let db = provider.connection_for(&tenant).await.unwrap();
    let products = TenantFilter::filter(product::Entity::find(), &tenant)
        .all(&db)
        .await
        .unwrap();
    axum::Json(ProductList {
        tenant: tenant.to_string(),
        products: products.into_iter().map(|p| p.name).collect(),
    })
}

struct SimpleSchemaMapping;

impl TenantSchemaMapping for SimpleSchemaMapping {
    async fn schema_for(&self, tenant: &TenantId) -> Result<String, tenant_core::TenantError> {
        // Schema name = "tenant_<id>"
        Ok(format!("tenant_{}", tenant.as_str()))
    }
}

#[tokio::test]
async fn schema_per_tenant_isolates_tenants() {
    let (_container, url) = start_postgres().await;
    let db = connect_pool1(&url).await;

    for schema_name in &["tenant_acme", "tenant_globex"] {
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            format!("CREATE SCHEMA \"{}\"", schema_name),
        ))
        .await
        .unwrap();

        db.execute(Statement::from_string(
            DbBackend::Postgres,
            format!("SET search_path TO \"{}\"", schema_name),
        ))
        .await
        .unwrap();

        create_products_table(&db).await;
    }

    db.execute(Statement::from_string(
        DbBackend::Postgres,
        "SET search_path TO \"tenant_acme\"".to_string(),
    ))
    .await
    .unwrap();
    seed_products(&db, "acme", &["Alpha", "Beta"]).await;

    db.execute(Statement::from_string(
        DbBackend::Postgres,
        "SET search_path TO \"tenant_globex\"".to_string(),
    ))
    .await
    .unwrap();
    seed_products(&db, "globex", &["Gamma", "Delta", "Epsilon"]).await;

    let shared_conn = connect_pool1(&url).await;
    let provider = Arc::new(SchemaPerTenantProvider::new(
        shared_conn,
        SimpleSchemaMapping,
    ));

    let app = Router::new()
        .route("/products", get(schema_list_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "acme")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "acme");
    assert_eq!(resp.products.len(), 2);
    assert!(resp.products.contains(&"Alpha".to_string()));
    assert!(resp.products.contains(&"Beta".to_string()));

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "globex")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "globex");
    assert_eq!(resp.products.len(), 3);
    assert!(resp.products.contains(&"Gamma".to_string()));
}

async fn schema_list_handler(
    CurrentTenant(tenant): CurrentTenant,
    State(provider): State<Arc<SchemaPerTenantProvider<SimpleSchemaMapping>>>,
) -> axum::Json<ProductList> {
    let db = provider.connection_for(&tenant).await.unwrap();
    let products = product::Entity::find().all(&db).await.unwrap();
    axum::Json(ProductList {
        tenant: tenant.to_string(),
        products: products.into_iter().map(|p| p.name).collect(),
    })
}

struct TestDatabaseMapping {
    host_port: u16,
}

impl TenantDatabaseMapping for TestDatabaseMapping {
    async fn url_for(&self, tenant: &TenantId) -> Result<String, tenant_core::TenantError> {
        Ok(format!(
            "postgres://postgres:postgres@127.0.0.1:{}/db_{}",
            self.host_port,
            tenant.as_str()
        ))
    }
}

#[tokio::test]
async fn database_per_tenant_isolates_tenants() {
    let (_container, url) = start_postgres().await;
    let host_port: u16 = url
        .split(':')
        .next_back()
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();

    let admin_db = connect(&url).await;

    for db_name in &["db_acme", "db_globex"] {
        admin_db
            .execute(Statement::from_string(
                DbBackend::Postgres,
                format!("CREATE DATABASE {}", db_name),
            ))
            .await
            .unwrap();

        let tenant_url = format!(
            "postgres://postgres:postgres@127.0.0.1:{}/{}",
            host_port, db_name
        );
        let tenant_db = connect(&tenant_url).await;
        create_products_table(&tenant_db).await;

        let tenant_name = db_name.strip_prefix("db_").unwrap();
        let products: Vec<&str> = match tenant_name {
            "acme" => vec!["Acme-Widget", "Acme-Gadget"],
            "globex" => vec!["Globex-Sprocket"],
            _ => vec![],
        };
        seed_products(&tenant_db, tenant_name, &products).await;
    }

    let mapping = TestDatabaseMapping { host_port };
    let provider = Arc::new(DatabasePerTenantProvider::new(url.clone(), mapping));

    let app = Router::new()
        .route("/products", get(dbpertenant_list_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "acme")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "acme");
    assert_eq!(resp.products.len(), 2);
    assert!(resp.products.contains(&"Acme-Widget".to_string()));

    // Globex hits db_globex
    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "globex")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "globex");
    assert_eq!(resp.products.len(), 1);
    assert_eq!(resp.products[0], "Globex-Sprocket");
}

async fn dbpertenant_list_handler(
    CurrentTenant(tenant): CurrentTenant,
    State(provider): State<Arc<DatabasePerTenantProvider<TestDatabaseMapping>>>,
) -> axum::Json<ProductList> {
    let db = provider.connection_for(&tenant).await.unwrap();
    let products = product::Entity::find().all(&db).await.unwrap();
    axum::Json(ProductList {
        tenant: tenant.to_string(),
        products: products.into_iter().map(|p| p.name).collect(),
    })
}

#[derive(Debug, Deserialize)]
struct CreateProduct {
    name: String,
}

#[tokio::test]
async fn discriminator_write_and_read_isolation() {
    let (_container, url) = start_postgres().await;
    let db = connect(&url).await;
    create_products_table(&db).await;

    let provider = Arc::new(DiscriminatorProvider::new(db));

    let app = Router::new()
        .route("/products", get(discriminator_list_handler))
        .route("/products", post(discriminator_create_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/products"))
        .header("X-Tenant-Id", "alpha")
        .json(&serde_json::json!({"name": "Alpha-Product"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED.as_u16());

    let resp = client
        .post(format!("{base}/products"))
        .header("X-Tenant-Id", "beta")
        .json(&serde_json::json!({"name": "Beta-Product"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED.as_u16());

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "alpha")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.products, vec!["Alpha-Product"]);

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "beta")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.products, vec!["Beta-Product"]);
}

async fn discriminator_create_handler(
    CurrentTenant(tenant): CurrentTenant,
    State(provider): State<Arc<DiscriminatorProvider>>,
    axum::Json(payload): axum::Json<CreateProduct>,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let db = provider.connection_for(&tenant).await.unwrap();
    let model = product::ActiveModel {
        name: Set(payload.name.clone()),
        tenant_id: Set(tenant.to_string()),
        ..Default::default()
    };
    let inserted = model.insert(&db).await.unwrap();
    (
        StatusCode::CREATED,
        axum::Json(serde_json::json!({
            "id": inserted.id,
            "name": inserted.name,
            "tenant_id": inserted.tenant_id,
        })),
    )
}

#[tokio::test]
async fn missing_tenant_header_returns_400() {
    let (_container, url) = start_postgres().await;
    let db = connect(&url).await;
    create_products_table(&db).await;

    let provider = Arc::new(DiscriminatorProvider::new(db));

    let app = Router::new()
        .route("/products", get(discriminator_list_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base}/products")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST.as_u16());
}

#[tokio::test]
async fn multi_strategy_header_and_query_with_db() {
    let (_container, url) = start_postgres().await;
    let db = connect(&url).await;
    create_products_table(&db).await;
    seed_products(&db, "from-header", &["H-Product"]).await;
    seed_products(&db, "from-query", &["Q-Product"]).await;

    let provider = Arc::new(DiscriminatorProvider::new(db));

    let app = Router::new()
        .route("/products", get(discriminator_list_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .strategy(HttpTenantStrategy::Query)
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    // Resolve from header
    let resp: ProductList = client
        .get(format!("{base}/products"))
        .header("X-Tenant-Id", "from-header")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "from-header");
    assert_eq!(resp.products, vec!["H-Product"]);

    // Resolve from query param (no header)
    let resp: ProductList = client
        .get(format!("{base}/products?tenant_id=from-query"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "from-query");
    assert_eq!(resp.products, vec!["Q-Product"]);
}

#[tokio::test]
async fn default_tenant_fallback_with_db() {
    let (_container, url) = start_postgres().await;
    let db = connect(&url).await;
    create_products_table(&db).await;
    seed_products(&db, "public", &["Public-Item"]).await;

    let provider = Arc::new(DiscriminatorProvider::new(db));

    let app = Router::new()
        .route("/products", get(discriminator_list_handler))
        .layer(TenantLayer::from_config(
            &HttpTenantConfig::builder()
                .strategy(HttpTenantStrategy::Header)
                .default_tenant("public")
                .build(),
        ))
        .with_state(provider);

    let base = serve(app).await;
    let client = reqwest::Client::new();

    let resp: ProductList = client
        .get(format!("{base}/products"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp.tenant, "public");
    assert_eq!(resp.products, vec!["Public-Item"]);
}

#[tokio::test]
async fn database_per_tenant_caches_connections() {
    let (_container, url) = start_postgres().await;
    let host_port: u16 = url
        .split(':')
        .next_back()
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();

    let admin_db = connect(&url).await;

    admin_db
        .execute(Statement::from_string(
            DbBackend::Postgres,
            "CREATE DATABASE db_cached".to_string(),
        ))
        .await
        .unwrap();

    let mapping = TestDatabaseMapping { host_port };
    let provider = DatabasePerTenantProvider::new(url.clone(), mapping);

    let tid = TenantId::new("cached").unwrap();

    let _conn1 = provider.connection_for(&tid).await.unwrap();
    let _conn2 = provider.connection_for(&tid).await.unwrap();

    let admin = provider.any_connection().await.unwrap();
    admin
        .execute(Statement::from_string(
            DbBackend::Postgres,
            "SELECT 1".to_string(),
        ))
        .await
        .unwrap();
}
