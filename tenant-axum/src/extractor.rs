use axum::extract::FromRequestParts;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use tenant_core::error::TenantError;
use tenant_core::tenant::{MultiTenancyStrategy, TenantId};

/// Rich tenant context inserted into request extensions by the middleware.
///
/// Contains the resolved tenant identity plus metadata about *how* it was
/// resolved. Use this when handlers need more than just the tenant ID.
///
/// ```rust,ignore
/// use tenant_axum::TenantContext;
///
/// async fn handler(ctx: TenantContext) -> String {
///     format!(
///         "Tenant {} resolved via {} (strategy: {:?})",
///         ctx.tenant_id(),
///         ctx.resolved_by(),
///         ctx.strategy(),
///     )
/// }
/// ```
#[derive(Debug, Clone)]
pub struct TenantContext {
    tenant_id: TenantId,
    /// Name of the resolver that produced the tenant (e.g. "HeaderTenantResolver").
    resolved_by: String,
    /// The multi-tenancy strategy in effect, if known.
    strategy: Option<MultiTenancyStrategy>,
}

impl TenantContext {
    /// Create a new `TenantContext`.
    pub fn new(
        tenant_id: TenantId,
        resolved_by: impl Into<String>,
        strategy: Option<MultiTenancyStrategy>,
    ) -> Self {
        Self {
            tenant_id,
            resolved_by: resolved_by.into(),
            strategy,
        }
    }

    /// The resolved tenant identifier.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Name of the resolver or mechanism that produced the tenant.
    pub fn resolved_by(&self) -> &str {
        &self.resolved_by
    }

    /// The multi-tenancy strategy in effect (if configured).
    pub fn strategy(&self) -> Option<MultiTenancyStrategy> {
        self.strategy
    }
}

impl<S> FromRequestParts<S> for TenantContext
where
    S: Send + Sync,
{
    type Rejection = TenantRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<TenantContext>()
            .cloned()
            .ok_or(TenantRejection(TenantError::MissingTenant))
    }
}

/// Axum extractor that yields the resolved [`TenantId`] from request
/// extensions.
///
/// The `TenantLayer` middleware must be applied for this to work.
///
/// ```rust,ignore
/// async fn handler(CurrentTenant(tenant): CurrentTenant) -> String {
///     format!("Hello, tenant {}", tenant)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CurrentTenant(pub TenantId);

impl std::ops::Deref for CurrentTenant {
    type Target = TenantId;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Rejection type used by [`CurrentTenant`] extractor, wrapping
/// [`TenantError`].
#[derive(Debug)]
pub struct TenantRejection(pub TenantError);

impl From<TenantError> for TenantRejection {
    fn from(e: TenantError) -> Self {
        Self(e)
    }
}

impl std::fmt::Display for TenantRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl IntoResponse for TenantRejection {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            TenantError::MissingTenant => http::StatusCode::BAD_REQUEST,
            TenantError::InvalidTenant(_) => http::StatusCode::BAD_REQUEST,
            TenantError::TenantNotFound(_) => http::StatusCode::NOT_FOUND,
            _ => http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.0.to_string()).into_response()
    }
}

impl<S> FromRequestParts<S> for CurrentTenant
where
    S: Send + Sync,
{
    type Rejection = TenantRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<TenantContext>()
            .map(|ctx| CurrentTenant(ctx.tenant_id().clone()))
            .ok_or(TenantRejection(TenantError::MissingTenant))
    }
}
