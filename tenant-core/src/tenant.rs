use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies a tenant. Wraps a validated, non-empty string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(String);

impl TenantId {
    /// Create a new `TenantId`. Returns `None` if the value is empty or
    /// contains only whitespace.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let s = value.into();
        if s.trim().is_empty() {
            None
        } else {
            Some(Self(s))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for TenantId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The three standard multi-tenancy strategies, mirroring Hibernate's
/// `MultiTenancyStrategy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiTenancyStrategy {
    /// Each tenant has its own database.
    Database,
    /// Each tenant has its own schema within a shared database.
    Schema,
    /// All tenants share the same database and tables, separated by a
    /// discriminator column.
    Discriminator,
}
