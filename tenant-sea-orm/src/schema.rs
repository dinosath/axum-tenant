use std::future::Future;
use std::time::{Duration, Instant};

use crate::connection::TenantConnectionProvider;
use dashmap::DashMap;
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
};
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
///
/// Each tenant receives a **dedicated connection** (pool of size 1) with the
/// search path pinned to its schema. This prevents cross-tenant data leakage
/// from concurrent requests.
///
/// # Backend support
///
/// - **Postgres**: Uses `SET search_path TO "schema_name"`
/// - **MySQL**: Uses `USE schema_name` (schemas = databases in MySQL)
/// - **SQLite**: Not supported (SQLite has no schema concept)
pub struct SchemaPerTenantProvider<M: TenantSchemaMapping> {
    base_url: String,
    mapping: M,
    schema_cache: DashMap<TenantId, String>,
    /// Per-tenant connection with schema already set.
    connections: DashMap<TenantId, (DatabaseConnection, Instant)>,
    /// Shared connection for `any_connection()` (no schema pinning).
    shared_connection: DatabaseConnection,
    /// TTL for cached tenant connections.
    ttl: Duration,
}

impl<M: TenantSchemaMapping> SchemaPerTenantProvider<M> {
    pub fn new(shared_connection: DatabaseConnection, mapping: M) -> Self {
        Self {
            base_url: String::new(),
            mapping,
            schema_cache: DashMap::new(),
            connections: DashMap::new(),
            shared_connection,
            ttl: Duration::from_secs(300),
        }
    }

    /// Create a provider with an explicit database URL for creating per-tenant
    /// connections.
    pub fn with_url(
        url: impl Into<String>,
        shared_connection: DatabaseConnection,
        mapping: M,
    ) -> Self {
        Self {
            base_url: url.into(),
            mapping,
            schema_cache: DashMap::new(),
            connections: DashMap::new(),
            shared_connection,
            ttl: Duration::from_secs(300),
        }
    }

    /// Set the TTL for cached per-tenant connections.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    async fn get_or_create_connection(
        &self,
        tenant: &TenantId,
        schema: &str,
    ) -> Result<DatabaseConnection, TenantError> {
        // Check cache and TTL
        if let Some(entry) = self.connections.get(tenant) {
            let (conn, created_at) = entry.value();
            if created_at.elapsed() < self.ttl {
                return Ok(conn.clone());
            }
            // Expired — drop and recreate below
            drop(entry);
            self.connections.remove(tenant);
        }

        // Create a dedicated connection (pool of 1) for this tenant
        let conn = if self.base_url.is_empty() {
            // Fallback: use shared connection but set schema
            // (not concurrency-safe with pool > 1 — callers should
            // prefer `with_url()` constructor)
            let backend = self.shared_connection.get_database_backend();
            let sql = schema_sql(backend, schema)?;
            self.shared_connection
                .execute(Statement::from_string(backend, sql))
                .await
                .map_err(|e| TenantError::SchemaError(e.to_string()))?;
            self.shared_connection.clone()
        } else {
            let mut opts = ConnectOptions::new(&self.base_url);
            opts.max_connections(1).min_connections(1);
            let conn = Database::connect(opts)
                .await
                .map_err(|e| TenantError::ConnectionError(e.to_string()))?;

            let backend = conn.get_database_backend();
            let sql = schema_sql(backend, schema)?;
            conn.execute(Statement::from_string(backend, sql))
                .await
                .map_err(|e| TenantError::SchemaError(e.to_string()))?;
            conn
        };

        self.connections
            .insert(tenant.clone(), (conn.clone(), Instant::now()));
        tracing::info!(tenant = %tenant, schema = %schema, "Created schema-scoped connection");
        Ok(conn)
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

        self.get_or_create_connection(tenant, &schema).await
    }

    async fn any_connection(&self) -> Result<DatabaseConnection, TenantError> {
        Ok(self.shared_connection.clone())
    }
}

/// Build the SQL statement to switch the active schema.
fn schema_sql(backend: DbBackend, schema: &str) -> Result<String, TenantError> {
    let quoted = quote_identifier(schema)?;
    match backend {
        DbBackend::Postgres => Ok(format!("SET search_path TO {}", quoted)),
        DbBackend::MySql => Ok(format!("USE {}", quoted)),
        DbBackend::Sqlite => Err(TenantError::SchemaError(
            "SQLite does not support schemas".into(),
        )),
    }
}

/// Quote a SQL identifier. Only allows alphanumeric + underscore to prevent
/// injection. Returns an error instead of panicking on invalid input.
fn quote_identifier(id: &str) -> Result<String, TenantError> {
    if id.is_empty() {
        return Err(TenantError::SchemaError(
            "SQL identifier cannot be empty".into(),
        ));
    }
    if !id.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(TenantError::SchemaError(format!(
            "Invalid SQL identifier (only alphanumeric and underscore allowed): {id}"
        )));
    }
    Ok(format!("\"{}\"", id))
}
