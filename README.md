# axum-tenant

[![CI](https://github.com/dinosath/axum-tenant/actions/workflows/ci.yml/badge.svg)](https://github.com/dinosath/axum-tenant/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/dinosath/axum-tenant/branch/main/graph/badge.svg)](https://codecov.io/gh/dinosath/axum-tenant)

A modular multi-tenancy framework for Rust built on **Axum** and **SeaORM**.

## Architecture

```text
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

## Multi-Tenancy Strategies

| Strategy | Isolation | Description |
|---|---|---|
| **Database** | Strongest | Each tenant gets its own database |
| **Schema** | Strong | Each tenant gets its own schema in a shared DB |
| **Discriminator** | Application-level | All tenants share tables; rows separated by `tenant_id` column |

## Quick Start

```toml
[dependencies]
axum-tenant = { version = "0.1", features = ["axum", "sea-orm"] }
```

To use **SeaORM 2.0** (currently `2.0.0-rc.38`) instead of SeaORM 1.x:

```toml
[dependencies]
axum-tenant = { version = "0.1", features = ["axum", "sea-orm-2"] }
```

Or depend on individual crates:

```toml
[dependencies]
tenant-core = "0.1"
tenant-axum = "0.1"
tenant-sea-orm = "0.1"
```

### Header-Based Tenant Resolution (simplest)

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

### Config-Driven Resolution

```rust
use tenant_axum::config::{HttpTenantConfig, HttpTenantStrategy};
use tenant_axum::TenantLayer;

let config = HttpTenantConfig::builder()
    .strategy(HttpTenantStrategy::Header)
    .strategy(HttpTenantStrategy::Cookie)
    .default_tenant("public")
    .build();

let app = Router::new()
    .route("/hello", get(handler))
    .layer(TenantLayer::from_config(&config));
```

### Database-per-Tenant Strategy

```rust
use tenant_sea_orm::{DatabasePerTenantProvider, TenantDatabaseMapping};
use tenant_core::{TenantId, TenantError};

struct MyMapping;

impl TenantDatabaseMapping for MyMapping {
    async fn url_for(&self, tenant: &TenantId) -> Result<String, TenantError> {
        Ok(format!("postgres://user:pass@localhost/{}", tenant.as_str()))
    }
}

let provider = DatabasePerTenantProvider::new(
    "postgres://user:pass@localhost/master",
    MyMapping,
);
```

### Schema-per-Tenant Strategy

```rust
use tenant_sea_orm::{SchemaPerTenantProvider, TenantSchemaMapping};

struct MySchemaMapping;

impl TenantSchemaMapping for MySchemaMapping {
    async fn schema_for(&self, tenant: &TenantId) -> Result<String, TenantError> {
        Ok(format!("tenant_{}", tenant.as_str()))
    }
}

let provider = SchemaPerTenantProvider::with_url(
    "postgres://user:pass@localhost/shared_db",
    db_connection,
    MySchemaMapping,
);
```

### Discriminator Strategy (Shared Database)

```rust
use tenant_sea_orm::{DiscriminatorProvider, TenantFilter, TenantAware};

let provider = DiscriminatorProvider::new(db_connection);

// In your handlers, filter queries:
let products = TenantFilter::filter(Product::find(), &tenant)
    .all(&db)
    .await?;
```

## Tenant Resolvers

| Resolver | Source | Example |
|---|---|---|
| `HeaderTenantResolver` | HTTP header (default: `X-Tenant-Id`) | `curl -H "X-Tenant-Id: acme" ...` |
| `PathTenantResolver` | URL path segment | `/acme/api/resource` |
| `SubdomainTenantResolver` | Host subdomain (requires ≥3 segments) | `acme.example.com` |
| `QueryParamTenantResolver` | Query parameter (default: `tenant_id`) | `?tenant_id=acme` |
| `CookieTenantResolver` | Cookie (default: `tenant_cookie`) | `Cookie: tenant_cookie=acme` |
| `JwtTenantResolver` | JWT Bearer token claim | `Authorization: Bearer <jwt>` |
| `DefaultTenantResolver` | Hardcoded fallback | Always returns configured value |

## SeaORM Entity Integration (Discriminator)

```rust
use sea_orm::entity::prelude::*;
use tenant_sea_orm::{TenantAware, TenantFilter};

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "product")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub tenant_id: String,
    pub name: String,
}

impl TenantAware for Entity {
    fn tenant_column() -> Column {
        Column::TenantId
    }
}

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
| Servlet Filter (setting tenant) | `TenantLayer` / `TenantMiddleware` |

## Features

| Feature | Default | Description |
|---|---|---|
| `axum` | ✓ | Axum middleware, extractors, HTTP resolvers |
| `sea-orm` | ✓ | SeaORM 1.x connection providers and query filtering |
| `sea-orm-2` | | SeaORM 2.0.0-rc.38 support (mutually exclusive with `sea-orm`) |
| `full` | | Enables all features |

## Running the Examples

```bash
cargo run --example basic --features axum
curl -H "X-Tenant-Id: acme" http://localhost:3000/hello
# → Hello from tenant: acme
```

## License

MIT
