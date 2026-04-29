use std::future::Future;

use crate::connection::TenantConnectionProvider;
use dashmap::DashMap;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tenant_core::error::TenantError;
use tenant_core::tenant::TenantId;

/// Resolves a tenant to a database connection URL.
///
/// Implement this to look up connection details for each tenant (from a
/// master database, config file, etc.).
pub trait TenantDatabaseMapping: Send + Sync + 'static {
    /// Return the database URL for the given tenant.
    fn url_for(
        &self,
        tenant: &TenantId,
    ) -> impl Future<Output = Result<String, TenantError>> + Send;
}

/// Database-per-tenant strategy: each tenant has its own database.
///
/// Analogous to the `DynamicDataSourceBasedMultiTenantConnectionProvider`
/// from the Hibernate / Spring Boot reference implementation.
///
/// Connections are cached per-tenant using a concurrent map.
pub struct DatabasePerTenantProvider<M: TenantDatabaseMapping> {
    mapping: M,
    cache: DashMap<TenantId, DatabaseConnection>,
    default_url: String,
}

impl<M: TenantDatabaseMapping> DatabasePerTenantProvider<M> {
    /// `default_url` is used for `any_connection()` (admin / bootstrap).
    pub fn new(default_url: impl Into<String>, mapping: M) -> Self {
        Self {
            mapping,
            cache: DashMap::new(),
            default_url: default_url.into(),
        }
    }
}

impl<M: TenantDatabaseMapping> TenantConnectionProvider for DatabasePerTenantProvider<M> {
    async fn connection_for(&self, tenant: &TenantId) -> Result<DatabaseConnection, TenantError> {
        if let Some(conn) = self.cache.get(tenant) {
            return Ok(conn.value().clone());
        }

        let url = self.mapping.url_for(tenant).await?;
        let opts = ConnectOptions::new(&url);
        let conn = Database::connect(opts)
            .await
            .map_err(|e| TenantError::ConnectionError(e.to_string()))?;

        self.cache.insert(tenant.clone(), conn.clone());
        tracing::info!(tenant = %tenant, "Created new database connection");
        Ok(conn)
    }

    async fn any_connection(&self) -> Result<DatabaseConnection, TenantError> {
        let opts = ConnectOptions::new(&self.default_url);
        Database::connect(opts)
            .await
            .map_err(|e| TenantError::ConnectionError(e.to_string()))
    }
}
