use crate::tenant::TenantId;

/// Per-request tenant context.
///
/// A request-scoped container that
/// holds the resolved tenant identifier so it can be accessed from any layer
/// (HTTP handlers, ORM hooks, background jobs, etc.).
///
/// In Rust this is typically stored in request extensions or task-locals rather
/// than CDI scopes.
pub trait TenantContext: Send + Sync {
    fn tenant_id(&self) -> Option<&TenantId>;
    fn set_tenant_id(&mut self, id: TenantId);
    fn clear(&mut self);
}

/// Simple in-memory implementation of [`TenantContext`].
#[derive(Debug, Clone, Default)]
pub struct DefaultTenantContext {
    tenant_id: Option<TenantId>,
}

impl DefaultTenantContext {
    pub fn new() -> Self {
        Self { tenant_id: None }
    }

    pub fn with_tenant(id: TenantId) -> Self {
        Self {
            tenant_id: Some(id),
        }
    }
}

impl TenantContext for DefaultTenantContext {
    fn tenant_id(&self) -> Option<&TenantId> {
        self.tenant_id.as_ref()
    }

    fn set_tenant_id(&mut self, id: TenantId) {
        self.tenant_id = Some(id);
    }

    fn clear(&mut self) {
        self.tenant_id = None;
    }
}
