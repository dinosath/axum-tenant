use crate::error::TenantError;
use crate::tenant::TenantId;
use std::any::{Any, TypeId};
use std::future::Future;
use std::pin::Pin;

/// Framework-agnostic resolution context.
///
/// Provides typed access
/// to transport-level details (HTTP headers, gRPC metadata, etc.) without
/// coupling the resolver interface to any specific framework.
pub trait ResolutionContext: Send + Sync {
    /// Retrieve a value by type id as `Any`. Override this to support typed
    /// access via [`get`](ResolutionContextExt::get).
    fn get_any(&self, _type_id: TypeId) -> Option<&dyn Any> {
        None
    }

    /// Convenience: get a header value by name. Transports that support
    /// key-value headers should implement this.
    fn header(&self, _name: &str) -> Option<&str> {
        None
    }

    /// Convenience: get the request URI path.
    fn path(&self) -> Option<&str> {
        None
    }

    /// Convenience: get the raw query string (without the leading `?`).
    fn query(&self) -> Option<&str> {
        None
    }
}

/// Extension trait providing typed access to [`ResolutionContext`] values.
pub trait ResolutionContextExt {
    fn get<T: 'static>(&self) -> Option<&T>;
}

impl<C: ResolutionContext + ?Sized> ResolutionContextExt for C {
    fn get<T: 'static>(&self) -> Option<&T> {
        self.get_any(TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
    }
}

/// Resolves the current tenant from a [`ResolutionContext`].
///
/// Analogous to Hibernate's `CurrentTenantIdentifierResolver`.
///
/// Implementations should be stateless; all request-specific data comes from
/// the context.
pub trait TenantResolver: Send + Sync + 'static {
    /// Attempt to resolve a tenant. Returns `Ok(None)` if this resolver
    /// cannot determine the tenant (the next resolver in the chain will be
    /// tried).
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError>;

    /// Optional name for logging / diagnostics.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

/// Chains multiple [`TenantResolver`]s and returns the first successful
/// result.
///
/// Chains multiple resolvers, returning the first successful result.
pub struct CompositeTenantResolver {
    resolvers: Vec<Box<dyn TenantResolver>>,
}

impl CompositeTenantResolver {
    pub fn new() -> Self {
        Self {
            resolvers: Vec::new(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, resolver: impl TenantResolver) -> Self {
        self.resolvers.push(Box::new(resolver));
        self
    }

    pub fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        for resolver in &self.resolvers {
            match resolver.resolve(ctx) {
                Ok(Some(id)) => return Ok(Some(id)),
                Ok(None) => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }
}

impl Default for CompositeTenantResolver {
    fn default() -> Self {
        Self::new()
    }
}

// Allow CompositeTenantResolver itself to be used as a TenantResolver
impl TenantResolver for CompositeTenantResolver {
    fn resolve(&self, ctx: &dyn ResolutionContext) -> Result<Option<TenantId>, TenantError> {
        CompositeTenantResolver::resolve(self, ctx)
    }

    fn name(&self) -> &str {
        "CompositeTenantResolver"
    }
}

// ─── Async TenantResolver ────────────────────────────────────────────

/// Async variant of [`TenantResolver`] for resolvers that require async
/// operations (e.g., database-backed tenant registries, remote config
/// lookups).
///
/// Use this when the resolution logic needs to perform I/O that cannot be
/// done synchronously. The middleware will `.await` the result.
///
/// # Example
///
/// ```rust,ignore
/// use tenant_core::resolver::AsyncTenantResolver;
///
/// struct DatabaseTenantResolver { /* ... */ }
///
/// impl AsyncTenantResolver for DatabaseTenantResolver {
///     fn resolve<'a>(
///         &'a self,
///         ctx: &'a dyn ResolutionContext,
///     ) -> Pin<Box<dyn Future<Output = Result<Option<TenantId>, TenantError>> + Send + 'a>> {
///         Box::pin(async move {
///             let header = ctx.header("x-tenant-id").unwrap_or_default();
///             let tenant = self.lookup_in_db(header).await?;
///             Ok(tenant)
///         })
///     }
/// }
/// ```
pub trait AsyncTenantResolver: Send + Sync + 'static {
    /// Asynchronously resolve the current tenant.
    fn resolve<'a>(
        &'a self,
        ctx: &'a dyn ResolutionContext,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TenantId>, TenantError>> + Send + 'a>>;

    /// Optional name for logging / diagnostics.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

/// Blanket implementation: every sync `TenantResolver` is also an
/// `AsyncTenantResolver`.
impl<T: TenantResolver> AsyncTenantResolver for T {
    fn resolve<'a>(
        &'a self,
        ctx: &'a dyn ResolutionContext,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TenantId>, TenantError>> + Send + 'a>> {
        Box::pin(std::future::ready(TenantResolver::resolve(self, ctx)))
    }

    fn name(&self) -> &str {
        TenantResolver::name(self)
    }
}
