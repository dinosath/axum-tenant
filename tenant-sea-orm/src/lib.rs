//! # tenant-sea-orm
//!
//! SeaORM-specific integration for the `tenant-core` multi-tenancy traits.
//!
//! Provides:
//! - [`TenantConnectionProvider`] — async trait for obtaining per-tenant
//!   `DatabaseConnection`s.
//! - [`DatabasePerTenantProvider`] — database-per-tenant strategy with
//!   connection caching.
//! - [`SchemaPerTenantProvider`] — schema-per-tenant strategy using
//!   `SET search_path`.
//! - [`DiscriminatorProvider`] — shared-database strategy; data isolation
//!   via query filtering.
//! - [`TenantFilter`] and [`TenantAware`] — query-level discriminator
//!   column helpers.
//!
//! Bridges the core `TenantContext` to the ORM layer's multi-tenancy API.

#[cfg(feature = "sea-orm-2")]
extern crate sea_orm_next as sea_orm;

pub(crate) mod compat;
pub mod connection;
pub mod database;
pub mod discriminator;
pub mod filter;
pub mod middleware;
#[allow(unreachable_patterns)]
pub mod provisioning;
#[allow(unreachable_patterns)]
pub mod rls;
#[allow(unreachable_patterns)]
pub mod schema;

pub use connection::TenantConnectionProvider;
pub use database::{DatabasePerTenantProvider, TenantDatabaseMapping};
pub use discriminator::DiscriminatorProvider;
pub use filter::{TenantAware, TenantFilter};
pub use middleware::{TenantDb, TenantDbLayer};
pub use provisioning::TenantProvisioner;
pub use rls::RlsManager;
pub use schema::{SchemaPerTenantProvider, TenantSchemaMapping};

// Re-export core types users commonly need
pub use tenant_core::{TenantError, TenantId};
