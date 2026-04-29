//! Discriminator example: header-based tenant resolution with SeaORM
//! tenant-aware query filtering.
//!
//! NOTE: This is a structural example — it demonstrates the API surface
//! without requiring a real database at compile time.

use axum::{routing::get, Router};
use tenant_axum::config::HttpTenantConfig;
use tenant_axum::{CurrentTenant, TenantLayer};
use tenant_sea_orm::TenantFilter;

mod entity {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "product")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub tenant_id: String,
        pub name: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}

    impl tenant_sea_orm::TenantAware for Entity {
        fn tenant_column() -> Column {
            Column::TenantId
        }
    }
}

async fn list_products(CurrentTenant(tenant): CurrentTenant) -> String {
    // In a real app you'd extract `DatabaseConnection` from state/extensions.
    // Here we just show how the filter is composed.
    let _query = TenantFilter::filter(<entity::Entity as sea_orm::EntityTrait>::find(), &tenant);
    format!("Would list products for tenant: {tenant}")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = HttpTenantConfig::default();

    let app = Router::new()
        .route("/products", get(list_products))
        .layer(TenantLayer::from_config(&config));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
