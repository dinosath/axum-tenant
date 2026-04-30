use std::future::Future;
use std::time::{Duration, Instant};

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
/// Connections are cached per-tenant using a concurrent map with TTL-based
/// eviction. Expired entries are lazily evicted on the next access.
///
/// # Backend support
///
/// Works with any SeaORM-supported backend (Postgres, MySQL, SQLite).
/// The `TenantDatabaseMapping` implementation is responsible for returning
/// the correct connection URL for each backend.
pub struct DatabasePerTenantProvider<M: TenantDatabaseMapping> {
    mapping: M,
    cache: DashMap<TenantId, CachedConnection>,
    default_connection: DashMap<(), DatabaseConnection>,
    default_url: String,
    /// Maximum number of cached tenant connections.
    max_connections: usize,
    /// Time-to-live for cached connections.
    ttl: Duration,
}

struct CachedConnection {
    connection: DatabaseConnection,
    created_at: Instant,
}

impl<M: TenantDatabaseMapping> DatabasePerTenantProvider<M> {
    /// `default_url` is used for `any_connection()` (admin / bootstrap).
    pub fn new(default_url: impl Into<String>, mapping: M) -> Self {
        Self {
            mapping,
            cache: DashMap::new(),
            default_connection: DashMap::new(),
            default_url: default_url.into(),
            max_connections: 100,
            ttl: Duration::from_secs(600),
        }
    }

    /// Set the maximum number of cached tenant connections.
    /// When exceeded, the oldest entries are evicted.
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Set the TTL for cached connections. Expired connections are lazily
    /// removed on the next access.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Remove expired entries from the cache.
    pub fn evict_expired(&self) {
        self.cache
            .retain(|_, entry| entry.created_at.elapsed() < self.ttl);
    }

    /// Remove a specific tenant's cached connection.
    pub fn evict_tenant(&self, tenant: &TenantId) {
        self.cache.remove(tenant);
    }

    /// Returns the number of currently cached connections.
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }

    /// Evict the oldest entry if we've exceeded `max_connections`.
    fn evict_if_full(&self) {
        if self.cache.len() >= self.max_connections {
            // Find and remove the oldest entry
            let oldest = self
                .cache
                .iter()
                .min_by_key(|entry| entry.value().created_at)
                .map(|entry| entry.key().clone());
            if let Some(key) = oldest {
                self.cache.remove(&key);
                tracing::debug!(tenant = %key, "Evicted oldest cached connection");
            }
        }
    }
}

impl<M: TenantDatabaseMapping> TenantConnectionProvider for DatabasePerTenantProvider<M> {
    async fn connection_for(&self, tenant: &TenantId) -> Result<DatabaseConnection, TenantError> {
        // Check cache with TTL validation
        if let Some(entry) = self.cache.get(tenant) {
            if entry.value().created_at.elapsed() < self.ttl {
                return Ok(entry.value().connection.clone());
            }
            // Expired — remove and recreate
            drop(entry);
            self.cache.remove(tenant);
        }

        let url = self.mapping.url_for(tenant).await?;
        let opts = ConnectOptions::new(&url);
        let conn = Database::connect(opts)
            .await
            .map_err(|e| TenantError::ConnectionError(e.to_string()))?;

        // Evict oldest if at capacity
        self.evict_if_full();

        self.cache.insert(
            tenant.clone(),
            CachedConnection {
                connection: conn.clone(),
                created_at: Instant::now(),
            },
        );
        tracing::info!(tenant = %tenant, "Created new database connection");
        Ok(conn)
    }

    async fn any_connection(&self) -> Result<DatabaseConnection, TenantError> {
        // Cache the default connection to avoid creating a new one on every call
        if let Some(conn) = self.default_connection.get(&()) {
            return Ok(conn.value().clone());
        }

        let opts = ConnectOptions::new(&self.default_url);
        let conn = Database::connect(opts)
            .await
            .map_err(|e| TenantError::ConnectionError(e.to_string()))?;
        self.default_connection.insert((), conn.clone());
        Ok(conn)
    }
}
