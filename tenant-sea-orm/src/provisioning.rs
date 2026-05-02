//! Tenant provisioning — create and destroy databases/schemas for tenants.
//!
//! Provides helpers for onboarding new tenants (creating their isolated
//! database or schema) and offboarding (dropping them).
//!
//! # Example
//!
//! ```rust,ignore
//! use tenant_sea_orm::provisioning::{TenantProvisioner, ProvisioningConfig};
//!
//! let provisioner = TenantProvisioner::new(admin_connection);
//!
//! // Create schema for new tenant
//! provisioner.create_schema("tenant_acme").await?;
//!
//! // Or create an entire database
//! provisioner.create_database("tenant_acme_db").await?;
//! ```

#[allow(unused_imports)]
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tenant_core::error::TenantError;

use crate::compat;

/// Validates a SQL identifier (schema/database name) is safe.
fn validate_identifier(name: &str) -> Result<(), TenantError> {
    if name.is_empty() {
        return Err(TenantError::InvalidTenant(
            "Identifier cannot be empty".into(),
        ));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(TenantError::InvalidTenant(format!(
            "Invalid SQL identifier: {name} (only alphanumeric and _ allowed)"
        )));
    }
    if name.len() > 63 {
        return Err(TenantError::InvalidTenant(format!(
            "Identifier too long: {name} (max 63 characters)"
        )));
    }
    Ok(())
}

/// Provisions tenant databases and schemas.
///
/// Uses an **admin-level** connection to execute DDL commands. Ensure the
/// connection has the appropriate privileges (CREATE DATABASE, CREATE SCHEMA).
pub struct TenantProvisioner {
    admin_conn: DatabaseConnection,
}

impl TenantProvisioner {
    pub fn new(admin_conn: DatabaseConnection) -> Self {
        Self { admin_conn }
    }

    /// Create a new schema for a tenant.
    ///
    /// - **Postgres**: `CREATE SCHEMA IF NOT EXISTS "name"`
    /// - **MySQL**: `CREATE DATABASE IF NOT EXISTS name` (MySQL schemas = databases)
    /// - **SQLite**: Not supported
    pub async fn create_schema(&self, schema_name: &str) -> Result<(), TenantError> {
        validate_identifier(schema_name)?;
        let backend = self.admin_conn.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema_name),
            DbBackend::MySql => {
                format!("CREATE DATABASE IF NOT EXISTS `{}`", schema_name)
            }
            DbBackend::Sqlite => {
                return Err(TenantError::SchemaError(
                    "SQLite does not support schema provisioning".into(),
                ));
            }
            _ => {
                return Err(TenantError::SchemaError(
                    "Unsupported database backend".into(),
                ));
            }
        };

        compat::exec(&self.admin_conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::SchemaError(format!("Failed to create schema: {e}")))?;

        tracing::info!(schema = %schema_name, "Provisioned tenant schema");
        Ok(())
    }

    /// Drop a tenant schema (and all its contents).
    ///
    /// **WARNING**: This is destructive and irreversible.
    ///
    /// - **Postgres**: `DROP SCHEMA IF EXISTS "name" CASCADE`
    /// - **MySQL**: `DROP DATABASE IF EXISTS name`
    pub async fn drop_schema(&self, schema_name: &str) -> Result<(), TenantError> {
        validate_identifier(schema_name)?;
        let backend = self.admin_conn.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema_name)
            }
            DbBackend::MySql => format!("DROP DATABASE IF EXISTS `{}`", schema_name),
            DbBackend::Sqlite => {
                return Err(TenantError::SchemaError(
                    "SQLite does not support schema provisioning".into(),
                ));
            }
            _ => {
                return Err(TenantError::SchemaError(
                    "Unsupported database backend".into(),
                ));
            }
        };

        compat::exec(&self.admin_conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::SchemaError(format!("Failed to drop schema: {e}")))?;

        tracing::warn!(schema = %schema_name, "Dropped tenant schema");
        Ok(())
    }

    /// Create a new database for a tenant (database-per-tenant strategy).
    ///
    /// - **Postgres**: `CREATE DATABASE "name"`
    /// - **MySQL**: `CREATE DATABASE IF NOT EXISTS name`
    /// - **SQLite**: Creates a new file (no SQL needed — return the path)
    pub async fn create_database(&self, db_name: &str) -> Result<(), TenantError> {
        validate_identifier(db_name)?;
        let backend = self.admin_conn.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => format!("CREATE DATABASE \"{}\"", db_name),
            DbBackend::MySql => format!("CREATE DATABASE IF NOT EXISTS `{}`", db_name),
            DbBackend::Sqlite => {
                return Err(TenantError::ConfigError(
                    "SQLite databases are file-based; create the file directly".into(),
                ));
            }
            _ => {
                return Err(TenantError::ConfigError(
                    "Unsupported database backend".into(),
                ));
            }
        };

        compat::exec(&self.admin_conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::ConnectionError(format!("Failed to create database: {e}")))?;

        tracing::info!(database = %db_name, "Provisioned tenant database");
        Ok(())
    }

    /// Drop a tenant database.
    ///
    /// **WARNING**: This is destructive and irreversible.
    pub async fn drop_database(&self, db_name: &str) -> Result<(), TenantError> {
        validate_identifier(db_name)?;
        let backend = self.admin_conn.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => format!("DROP DATABASE IF EXISTS \"{}\"", db_name),
            DbBackend::MySql => format!("DROP DATABASE IF EXISTS `{}`", db_name),
            DbBackend::Sqlite => {
                return Err(TenantError::ConfigError(
                    "SQLite databases are file-based; delete the file directly".into(),
                ));
            }
            _ => {
                return Err(TenantError::ConfigError(
                    "Unsupported database backend".into(),
                ));
            }
        };

        compat::exec(&self.admin_conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::ConnectionError(format!("Failed to drop database: {e}")))?;

        tracing::warn!(database = %db_name, "Dropped tenant database");
        Ok(())
    }

    /// Check if a schema exists.
    pub async fn schema_exists(&self, schema_name: &str) -> Result<bool, TenantError> {
        validate_identifier(schema_name)?;
        let backend = self.admin_conn.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => format!(
                "SELECT 1 FROM information_schema.schemata WHERE schema_name = '{}'",
                schema_name
            ),
            DbBackend::MySql => format!(
                "SELECT 1 FROM information_schema.schemata WHERE schema_name = '{}'",
                schema_name
            ),
            DbBackend::Sqlite => {
                return Err(TenantError::SchemaError(
                    "SQLite does not support schemas".into(),
                ));
            }
            _ => {
                return Err(TenantError::SchemaError(
                    "Unsupported database backend".into(),
                ));
            }
        };

        let result = compat::query_one(&self.admin_conn, Statement::from_string(backend, sql))
            .await
            .map_err(|e| TenantError::SchemaError(format!("Failed to check schema: {e}")))?;

        Ok(result.is_some())
    }

    /// Run arbitrary SQL migrations within a specific schema context.
    ///
    /// Switches to the given schema, runs the SQL, then returns.
    /// Useful for running migrations per-tenant.
    pub async fn run_migration_sql(&self, schema_name: &str, sql: &str) -> Result<(), TenantError> {
        validate_identifier(schema_name)?;
        let backend = self.admin_conn.get_database_backend();

        // Switch to the schema
        let switch_sql = match backend {
            DbBackend::Postgres => format!("SET search_path TO \"{}\"", schema_name),
            DbBackend::MySql => format!("USE `{}`", schema_name),
            DbBackend::Sqlite => {
                return Err(TenantError::SchemaError(
                    "SQLite does not support schemas".into(),
                ));
            }
            _ => {
                return Err(TenantError::SchemaError(
                    "Unsupported database backend".into(),
                ));
            }
        };

        compat::exec(
            &self.admin_conn,
            Statement::from_string(backend, switch_sql),
        )
        .await
        .map_err(|e| TenantError::SchemaError(format!("Failed to switch schema: {e}")))?;

        // Run the migration
        compat::exec(
            &self.admin_conn,
            Statement::from_string(backend, sql.to_owned()),
        )
        .await
        .map_err(|e| TenantError::SchemaError(format!("Migration failed: {e}")))?;

        Ok(())
    }
}
