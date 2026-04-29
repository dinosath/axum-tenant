use thiserror::Error;

/// Errors that can occur in the multi-tenancy system.
#[derive(Error, Debug)]
pub enum TenantError {
    #[error("No tenant identifier found in the request")]
    MissingTenant,

    #[error("Invalid tenant identifier: {0}")]
    InvalidTenant(String),

    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    #[error("Database connection error: {0}")]
    ConnectionError(String),

    #[error("Schema switching error: {0}")]
    SchemaError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("{0}")]
    Other(String),
}
