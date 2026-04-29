//! # tenant-axum
//!
//! Axum-specific integration for the `tenant-core` multi-tenancy traits.
//!
//! Provides:
//! - [`HttpResolutionContext`] — adapts Axum's `http::request::Parts` into a
//!   `tenant_core::ResolutionContext`.
//! - Built-in HTTP resolvers: [`HeaderTenantResolver`],
//!   [`PathTenantResolver`], [`SubdomainTenantResolver`],
//!   [`QueryParamTenantResolver`].
//! - [`TenantLayer`] / [`TenantMiddleware`] — Tower middleware that resolves
//!   the tenant and injects `TenantId` into request extensions.
//! - [`CurrentTenant`] — Axum extractor for the resolved `TenantId`.

pub mod config;
pub mod context;
pub mod extractor;
pub mod middleware;
pub mod resolver;

pub use context::HttpResolutionContext;
pub use extractor::{CurrentTenant, TenantRejection};
pub use middleware::TenantLayer;
pub use resolver::{
    CookieTenantResolver, DefaultTenantResolver, HeaderTenantResolver, JwtTenantResolver,
    PathTenantResolver, QueryParamTenantResolver, SubdomainTenantResolver,
};

// Re-export core types users commonly need
pub use tenant_core::{
    CompositeTenantResolver, MultiTenancyStrategy, ResolutionContext, ResolutionContextExt,
    TenantContext, TenantError, TenantId, TenantResolver,
};
