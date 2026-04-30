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

pub mod connection;
pub mod database;
pub mod discriminator;
pub mod filter;
pub mod middleware;
pub mod provisioning;
pub mod rls;
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
