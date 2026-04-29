//! # axum-tenant
//!
//! A modular multi-tenancy framework for Rust, inspired by Hibernate ORM's
//! multi-tenancy support.
//!
//! ## Architecture
//!
//! - **`tenant-core`** — Framework-agnostic traits (`TenantResolver`,
//!   `TenantContext`, `ResolutionContext`) and value types (`TenantId`,
//!   `TenantError`, `MultiTenancyStrategy`).
//! - **`tenant-axum`** — Axum integration: HTTP resolvers, Tower middleware,
//!   and extractors.
//! - **`tenant-sea-orm`** — SeaORM integration: connection providers and
//!   query-level tenant filtering.
//!
//! Enable the features you need:
//!
//! ```toml
//! [dependencies]
//! axum-tenant = { version = "0.1", features = ["axum", "sea-orm"] }
//! ```
//!
//! Or use `features = ["full"]` to enable everything.

// Always re-export core
pub use tenant_core::*;

#[cfg(feature = "axum")]
pub mod axum {
    //! Axum integration — re-exports from `tenant-axum`.
    pub use tenant_axum::*;
}

#[cfg(feature = "sea-orm")]
pub mod orm {
    //! SeaORM integration — re-exports from `tenant-sea-orm`.
    pub use tenant_sea_orm::*;
}
