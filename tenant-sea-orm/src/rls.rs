//! Row Level Security (RLS) helpers for Postgres.
//!
//! Provides utilities to set up and activate Postgres RLS policies that
//! automatically filter rows by tenant. This is the strongest isolation
//! mechanism for the discriminator strategy — even if application code
//! forgets to filter, the database enforces isolation.
//!
//! # How it works
//!
//! 1. **Setup** (once per table): Enable RLS and create a policy that filters
//!    rows where `tenant_id = current_setting('app.tenant_id')`.
//! 2. **Per-request**: Before queries, set `app.tenant_id` on the connection.
//!
//! # Example
//!
//! ```rust,ignore
//! use tenant_sea_orm::rls::RlsManager;
//!
//! let rls = RlsManager::new(admin_conn);
//!
//! // One-time setup
//! rls.enable_rls("products", "tenant_id").await?;
//!
//! // Per-request (typically in middleware)
//! rls.set_tenant(&conn, &tenant_id).await?;
//! ```

#[allow(unused_imports)]
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tenant_core::error::TenantError;
use tenant_core::tenant::TenantId;

use crate::compat;

/// The Postgres session variable used to pass tenant identity to RLS policies.
pub const RLS_TENANT_VAR: &str = "app.tenant_id";

/// Manages Row Level Security policies for tenant isolation.
///
/// Only supported on **Postgres**. Operations on other backends return errors.
pub struct RlsManager {
    admin_conn: DatabaseConnection,
}

impl RlsManager {
    pub fn new(admin_conn: DatabaseConnection) -> Self {
        Self { admin_conn }
    }

    /// Enable RLS on a table and create a policy filtering by tenant column.
    ///
    /// Creates a policy named `tenant_isolation_{table}` that restricts
    /// all operations (SELECT, INSERT, UPDATE, DELETE) to rows where
    /// `{tenant_column} = current_setting('app.tenant_id')`.
    ///
    /// This is idempotent — safe to call multiple times.
    pub async fn enable_rls(
        &self,
        table_name: &str,
        tenant_column: &str,
    ) -> Result<(), TenantError> {
        Self::check_postgres(&self.admin_conn)?;
        validate_identifier(table_name)?;
        validate_identifier(tenant_column)?;

        let stmts = vec![
            format!("ALTER TABLE \"{table_name}\" ENABLE ROW LEVEL SECURITY"),
            format!("ALTER TABLE \"{table_name}\" FORCE ROW LEVEL SECURITY"),
            format!(
                "CREATE POLICY IF NOT EXISTS tenant_isolation_{table_name} \
                 ON \"{table_name}\" \
                 USING (\"{tenant_column}\" = current_setting('{RLS_TENANT_VAR}'))"
            ),
        ];

        let backend = self.admin_conn.get_database_backend();
        for sql in stmts {
            compat::exec(&self.admin_conn, Statement::from_string(backend, sql))
                .await
                .map_err(|e| TenantError::SchemaError(format!("RLS setup failed: {e}")))?;
        }

        tracing::info!(table = %table_name, "RLS enabled");
        Ok(())
    }

    /// Disable RLS on a table and drop the tenant isolation policy.
    pub async fn disable_rls(&self, table_name: &str) -> Result<(), TenantError> {
        Self::check_postgres(&self.admin_conn)?;
        validate_identifier(table_name)?;

        let stmts = vec![
            format!("DROP POLICY IF EXISTS tenant_isolation_{table_name} ON \"{table_name}\""),
            format!("ALTER TABLE \"{table_name}\" DISABLE ROW LEVEL SECURITY"),
        ];

        let backend = self.admin_conn.get_database_backend();
        for sql in stmts {
            compat::exec(&self.admin_conn, Statement::from_string(backend, sql))
                .await
                .map_err(|e| TenantError::SchemaError(format!("RLS disable failed: {e}")))?;
        }

        tracing::warn!(table = %table_name, "RLS disabled");
        Ok(())
    }

    /// Set the current tenant on a connection for RLS enforcement.
    ///
    /// Call this at the beginning of each request (typically in middleware)
    /// before executing any queries.
    ///
    /// ```rust,ignore
    /// rls.set_tenant(&conn, &tenant_id).await?;
    /// // All subsequent queries on `conn` are now filtered by tenant
    /// ```
    pub async fn set_tenant(
        conn: &DatabaseConnection,
        tenant: &TenantId,
    ) -> Result<(), TenantError> {
        Self::check_postgres(conn)?;
        let backend = conn.get_database_backend();
        let sql = format!(
            "SET LOCAL {} = '{}'",
            RLS_TENANT_VAR,
            tenant.as_str().replace('\'', "''")
        );
        compat::exec(conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::SchemaError(format!("Failed to set RLS tenant: {e}")))?;
        Ok(())
    }

    /// Reset the tenant variable (unset). Useful for admin operations.
    pub async fn reset_tenant(conn: &DatabaseConnection) -> Result<(), TenantError> {
        Self::check_postgres(conn)?;
        let backend = conn.get_database_backend();
        let sql = format!("RESET {}", RLS_TENANT_VAR);
        compat::exec(conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::SchemaError(format!("Failed to reset RLS tenant: {e}")))?;
        Ok(())
    }

    fn check_postgres(conn: &DatabaseConnection) -> Result<(), TenantError> {
        if conn.get_database_backend() != DbBackend::Postgres {
            return Err(TenantError::ConfigError(
                "Row Level Security is only supported on Postgres".into(),
            ));
        }
        Ok(())
    }
}

fn validate_identifier(name: &str) -> Result<(), TenantError> {
    if name.is_empty() {
        return Err(TenantError::InvalidTenant(
            "Identifier cannot be empty".into(),
        ));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(TenantError::InvalidTenant(format!(
            "Invalid SQL identifier: {name}"
        )));
    }
    Ok(())
}
