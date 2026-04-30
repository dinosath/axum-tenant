use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies a tenant. Wraps a validated, non-empty, trimmed string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TenantId(String);

impl TenantId {
    /// Create a new `TenantId`. Returns `None` if the value is empty or
    /// contains only whitespace. The value is trimmed before storage.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let s = value.into();
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(Self(trimmed))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for TenantId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TenantId::new(s)
            .ok_or_else(|| serde::de::Error::custom("TenantId cannot be empty or whitespace-only"))
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
