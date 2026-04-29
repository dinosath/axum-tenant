use axum::extract::FromRequestParts;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use tenant_core::error::TenantError;
use tenant_core::tenant::TenantId;

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
            .get::<TenantId>()
            .cloned()
            .map(CurrentTenant)
            .ok_or_else(|| TenantRejection(TenantError::MissingTenant))
    }
}
