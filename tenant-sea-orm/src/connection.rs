use std::future::Future;

use sea_orm::DatabaseConnection;
use tenant_core::error::TenantError;
use tenant_core::tenant::TenantId;

/// Provides tenant-specific database connections.
///
/// Analogous to Hibernate's `MultiTenantConnectionProvider`.
///
/// Implement this for your chosen multi-tenancy strategy.
pub trait TenantConnectionProvider: Send + Sync + 'static {
    /// Get a database connection for the given tenant.
    fn connection_for(
        &self,
        tenant: &TenantId,
    ) -> impl Future<Output = Result<DatabaseConnection, TenantError>> + Send;

    /// Get a connection not associated with any specific tenant (admin /
    /// bootstrap operations).
    fn any_connection(
        &self,
    ) -> impl Future<Output = Result<DatabaseConnection, TenantError>> + Send;
}
