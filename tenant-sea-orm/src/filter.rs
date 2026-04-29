use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, Select};
use tenant_core::tenant::TenantId;

/// Marker trait for SeaORM entities that carry a `tenant_id` discriminator
/// column.
///
/// Analogous to the `TenantAware` interface / `AbstractBaseEntity` from the
/// Hibernate / Spring Boot reference and Hibernate 6's `@TenantId`
/// annotation.
pub trait TenantAware: EntityTrait {
    /// The column that holds the tenant identifier (typically
    /// `Column::TenantId`).
    fn tenant_column() -> Self::Column;
}

/// Query-level tenant filter helper for the discriminator strategy.
///
/// Adds `WHERE tenant_id = ?` to SeaORM queries, enforcing data isolation.
pub struct TenantFilter;

impl TenantFilter {
    /// Apply a tenant filter to a `Select` query.
    ///
    /// ```rust,ignore
    /// let products = TenantFilter::filter(Product::find(), &tenant_id)
    ///     .all(&db)
    ///     .await?;
    /// ```
    pub fn filter<E>(query: Select<E>, tenant: &TenantId) -> Select<E>
    where
        E: TenantAware,
    {
        query.filter(E::tenant_column().eq(tenant.as_str()))
    }

    /// Build a standalone `Condition` for composing complex queries.
    pub fn condition<E>(tenant: &TenantId) -> Condition
    where
        E: TenantAware,
    {
        Condition::all().add(E::tenant_column().eq(tenant.as_str()))
    }
}
