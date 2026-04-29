//! # tenant-core
//!
//! Framework-agnostic core traits and types for multi-tenancy.
//!
//! This crate defines the foundational abstractions — `TenantId`,
//! `TenantResolver`, `TenantContext`, and `TenantConnectionProvider` — without
//! coupling to any web framework or ORM. Concrete implementations live in
//! companion crates (`tenant-axum`, `tenant-sea-orm`).

pub mod config;
pub mod context;
pub mod error;
pub mod resolver;
pub mod tenant;

pub use config::TenantConfig;
pub use context::TenantContext;
pub use error::TenantError;
pub use resolver::{
    CompositeTenantResolver, ResolutionContext, ResolutionContextExt, TenantResolver,
};
pub use tenant::{MultiTenancyStrategy, TenantId};
