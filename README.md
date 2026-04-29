# axum-tenant

A modular multi-tenancy framework for Rust
## Architecture

```
axum-tenant (facade crate — feature-gated re-exports)
├── tenant-core       Core traits & types — framework agnostic
├── tenant-axum       Axum integration — HTTP resolvers, Tower middleware, extractors
└── tenant-sea-orm    SeaORM integration — connection providers, tenant-aware filtering
```

| Crate | Purpose |
|---|---|
| `tenant-core` | `TenantResolver`, `ResolutionContext`, `TenantContext`, `TenantId`, `TenantError` |
| `tenant-axum` | `HeaderTenantResolver`, `PathTenantResolver`, `SubdomainTenantResolver`, `TenantLayer`, `CurrentTenant` |
| `tenant-sea-orm` | `TenantConnectionProvider`, `DatabasePerTenantProvider`, `SchemaPerTenantProvider`, `DiscriminatorProvider`, `TenantFilter` |

## Multi-Tenancy Strategies

- **Database per tenant** — each tenant gets its own database; connections are cached.
- **Schema per tenant** — shared database, each tenant gets its own schema (`SET search_path`).
- **Discriminator (shared table)** — all tenants share the same tables; data isolation via `WHERE tenant_id = ?`.

## Quick Start

```toml
[dependencies]
axum-tenant = { version = "0.1", features = ["axum", "sea-orm"] }
```

Or depend on individual crates:

```toml
[dependencies]
tenant-core = "0.1"
tenant-axum = "0.1"
tenant-sea-orm = "0.1"
```

### Example — header-based tenant resolution

```rust
use axum::{routing::get, Router};
use tenant_axum::{
    CompositeTenantResolver, CurrentTenant, HeaderTenantResolver, TenantLayer,
};

async fn handler(CurrentTenant(tenant): CurrentTenant) -> String {
    format!("Hello, tenant {tenant}")
}

#[tokio::main]
async fn main() {
    let resolver = CompositeTenantResolver::new()
        .add(HeaderTenantResolver::default());

    let app = Router::new()
        .route("/hello", get(handler))
        .layer(TenantLayer::new(resolver));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

```bash
curl -H "X-Tenant-Id: acme" http://localhost:3000/hello
# → Hello, tenant acme
```

## Features

| Feature | Default | Description |
|---|---|---|
| `axum` | ✓ | Axum middleware, extractors, HTTP resolvers |
| `sea-orm` | ✓ | SeaORM connection providers and query filtering |
| `full` | | Enables all features |

## License

MIT

A multi-tenancy crate for Rust built on **Axum** and **SeaORM**.

## Multi-Tenancy Strategies

| Strategy | Isolation |  Description |
|---|---|---|---|
| **Database** | Strongest | Each tenant gets its own database |
| **Schema** | Strong | Each tenant gets its own schema in a shared DB |
| **Discriminator** | Application-level | All tenants share tables; rows separated by `tenant_id` column |

## Architecture

```text
┌─────────────────────────────────────────────┐
│              Axum HTTP Request               │
└──────────────────┬──────────────────────────┘
                   │
         ┌─────────▼──────────┐
         │    TenantLayer     │  ← Tower middleware
         │  (TenantResolver)  │
         └─────────┬──────────┘
                   │ extracts TenantId
         ┌─────────▼───────────────────┐
         │  TenantConnectionProvider   │  ← provides DB conn
         └─────────┬───────────────────┘
                   │
    ┌──────────────┼──────────────┐
    │              │              │
    ▼              ▼              ▼
 Database       Schema      Discriminator
 Provider      Provider      Provider
 (per-tenant   (SET SCHEMA)  (shared conn
  conn pool)                 + TenantFilter)
```

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
axum-tenant = { path = "." }
axum = "0.8"
sea-orm = { version = "1", features = ["runtime-tokio-rustls", "sqlx-postgres"] }
tokio = { version = "1", features = ["full"] }
```

### 1. Header-Based Tenant Resolution (simplest)

```rust
use axum::{Router, routing::get};
use axum_tenant::{TenantLayer, HeaderTenantResolver, CurrentTenant};

async fn handler(CurrentTenant(tenant): CurrentTenant) -> String {
    format!("Hello, tenant {tenant}")
}

let app = Router::new()
    .route("/api/resource", get(handler))
    .layer(TenantLayer::new(HeaderTenantResolver::default()));
// Expects header: X-Tenant-Id: <tenant>
```

### 2. Database-per-Tenant Strategy

```rust
use axum_tenant::{
    TenantLayer, HeaderTenantResolver, TenantDb,
    sea_orm::DatabasePerTenantProvider,
};
use axum_tenant::sea_orm::database::TenantDatabaseMapping;
use axum_tenant::{TenantId, TenantError};

struct MyMapping;

impl TenantDatabaseMapping for MyMapping {
    async fn url_for(&self, tenant: &TenantId) -> Result<String, TenantError> {
        // Look up from master DB, config, etc.
        Ok(format!("postgres://user:pass@localhost/{}", tenant.as_str()))
    }
}

let provider = DatabasePerTenantProvider::new(
    "postgres://user:pass@localhost/master",
    MyMapping,
);

let app = Router::new()
    .route("/api/resource", get(handler))
    .layer(TenantLayer::with_connection_provider(
        HeaderTenantResolver::default(),
        provider,
    ));
```

### 3. Schema-per-Tenant Strategy

```rust
use axum_tenant::sea_orm::{SchemaPerTenantProvider, schema::TenantSchemaMapping};

struct MySchemaMapping;

impl TenantSchemaMapping for MySchemaMapping {
    async fn schema_for(&self, tenant: &TenantId) -> Result<String, TenantError> {
        Ok(format!("tenant_{}", tenant.as_str()))
    }
}

let provider = SchemaPerTenantProvider::new(db_connection, MySchemaMapping);
```

### 4. Discriminator Strategy (Shared Database)

```rust
use axum_tenant::sea_orm::{DiscriminatorProvider, TenantFilter};

// Connection provider: just wraps the shared connection
let provider = DiscriminatorProvider::new(db_connection);

// In your handlers, filter queries:
async fn list_products(
    CurrentTenant(tenant): CurrentTenant,
    TenantDb(db): TenantDb,
) -> Result<Json<Vec<product::Model>>, TenantError> {
    let products = TenantFilter::filter(Product::find(), &tenant)
        .all(&db)
        .await?;
    Ok(Json(products))
}
```

## Tenant Resolvers

| Resolver | Source | Example |
|---|---|---|
| `HeaderTenantResolver` | HTTP header (default: `X-Tenant-Id`) | `curl -H "X-Tenant-Id: acme" ...` |
| `PathTenantResolver` | URL path segment | `/acme/api/resource` |
| `SubdomainTenantResolver` | Host subdomain | `acme.example.com` |
| `QueryParamTenantResolver` | Query parameter (default: `tenant_id`) | `?tenant_id=acme` |

### Custom Resolver

```rust
use axum_tenant::{TenantResolver, TenantId, TenantError};
use http::request::Parts;

struct JwtTenantResolver;

impl TenantResolver for JwtTenantResolver {
    async fn resolve(&self, parts: &Parts) -> Result<TenantId, TenantError> {
        let auth = parts.headers
            .get("Authorization")
            .ok_or(TenantError::MissingTenant)?
            .to_str()
            .map_err(|_| TenantError::InvalidTenant("bad header".into()))?;
        
        // Decode JWT, extract tenant claim...
        let tenant_id = extract_tenant_from_jwt(auth)?;
        TenantId::new(tenant_id).ok_or(TenantError::MissingTenant)
    }
}
```

## SeaORM Entity Integration (Discriminator)

Make your entities tenant-aware by implementing the `TenantAware` trait:

```rust
use sea_orm::entity::prelude::*;
use axum_tenant::sea_orm::TenantFilter;

// Your SeaORM entity must have a tenant_id column
#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "product")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub tenant_id: String,
    pub name: String,
}

// Implement TenantAware to enable TenantFilter
impl axum_tenant::sea_orm::filter::TenantAware for Entity {
    fn tenant_column() -> Column {
        Column::TenantId
    }
}
```

Then use `TenantFilter` in queries:

```rust
// SELECT * FROM product WHERE tenant_id = 'acme'
let products = TenantFilter::filter(Entity::find(), &tenant)
    .all(&db)
    .await?;
```

## Concepts Mapping: Hibernate → axum-tenant

| Hibernate / Spring Boot | axum-tenant |
|---|---|
| `MultiTenancyStrategy` | `MultiTenancyStrategy` enum |
| `CurrentTenantIdentifierResolver` | `TenantResolver` trait |
| `MultiTenantConnectionProvider` | `TenantConnectionProvider` trait |
| `TenantContext` (ThreadLocal) | `TenantId` in Axum request extensions |
| `@TenantId` / `TenantAware` | `TenantAware` trait on SeaORM entities |
| Hibernate Filter | `TenantFilter::filter()` |
| `AbstractBaseEntity` | `TenantAware` trait |
| Servlet Filter (setting tenant) | `TenantLayer` / `TenantMiddleware` |
| `@PrePersist` listener | Set `tenant_id` field before `ActiveModel::insert()` |

## Running the Example

```bash
cargo run --example basic
# In another terminal:
curl -H "X-Tenant-Id: acme" http://localhost:3000/hello
# → Hello from tenant: acme
```

## License

MIT