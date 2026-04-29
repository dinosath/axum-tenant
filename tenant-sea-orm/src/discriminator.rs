use crate::connection::TenantConnectionProvider;
use sea_orm::DatabaseConnection;
use tenant_core::error::TenantError;
use tenant_core::tenant::TenantId;

/// Shared-database discriminator strategy.
///
/// All tenants share the same database and tables. Data isolation is enforced
/// at the query level via [`TenantFilter`](super::TenantFilter) and the
/// [`TenantAware`](super::TenantAware) entity trait.
///
/// For maximum safety, combine with Postgres Row Level Security.
pub struct DiscriminatorProvider {
    connection: DatabaseConnection,
}

impl DiscriminatorProvider {
    pub fn new(connection: DatabaseConnection) -> Self {
        Self { connection }
    }
}

impl TenantConnectionProvider for DiscriminatorProvider {
    async fn connection_for(&self, _tenant: &TenantId) -> Result<DatabaseConnection, TenantError> {
        Ok(self.connection.clone())
    }

    async fn any_connection(&self) -> Result<DatabaseConnection, TenantError> {
        Ok(self.connection.clone())
    }
}
