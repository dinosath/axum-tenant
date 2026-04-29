use std::future::Future;

use crate::connection::TenantConnectionProvider;
use dashmap::DashMap;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tenant_core::error::TenantError;
use tenant_core::tenant::TenantId;

/// Resolves a tenant to a database schema name.
pub trait TenantSchemaMapping: Send + Sync + 'static {
    fn schema_for(
        &self,
        tenant: &TenantId,
    ) -> impl Future<Output = Result<String, TenantError>> + Send;
}

/// Schema-per-tenant strategy: all tenants share a database, each gets its
/// own schema.
pub struct SchemaPerTenantProvider<M: TenantSchemaMapping> {
    shared_connection: DatabaseConnection,
    mapping: M,
    schema_cache: DashMap<TenantId, String>,
}

impl<M: TenantSchemaMapping> SchemaPerTenantProvider<M> {
    pub fn new(shared_connection: DatabaseConnection, mapping: M) -> Self {
        Self {
            shared_connection,
            mapping,
            schema_cache: DashMap::new(),
        }
    }

    async fn set_schema(&self, schema: &str) -> Result<(), TenantError> {
        let backend = self.shared_connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => format!("SET search_path TO {}", quote_identifier(schema)),
            DbBackend::MySql => format!("USE {}", quote_identifier(schema)),
            DbBackend::Sqlite => {
                return Err(TenantError::SchemaError(
                    "SQLite does not support schemas".into(),
                ))
            }
        };
        self.shared_connection
            .execute(Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::SchemaError(e.to_string()))?;
        Ok(())
    }
}

impl<M: TenantSchemaMapping> TenantConnectionProvider for SchemaPerTenantProvider<M> {
    async fn connection_for(&self, tenant: &TenantId) -> Result<DatabaseConnection, TenantError> {
        let schema = if let Some(s) = self.schema_cache.get(tenant) {
            s.clone()
        } else {
            let s = self.mapping.schema_for(tenant).await?;
            self.schema_cache.insert(tenant.clone(), s.clone());
            s
        };

        self.set_schema(&schema).await?;
        Ok(self.shared_connection.clone())
    }

    async fn any_connection(&self) -> Result<DatabaseConnection, TenantError> {
        Ok(self.shared_connection.clone())
    }
}

/// Quote a SQL identifier. Only allows alphanumeric + underscore to prevent
/// injection.
fn quote_identifier(id: &str) -> String {
    if !id.chars().all(|c| c.is_alphanumeric() || c == '_') {
        panic!("Invalid SQL identifier: {id}");
    }
    format!("\"{}\"", id)
}
