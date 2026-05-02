//! Internal compatibility helpers for sea-orm 1.x vs 2.x API differences.

use sea_orm::{ConnectionTrait, ExecResult, QueryResult, Statement};

/// Execute a statement, abstracting over the API difference between
/// sea-orm 1.x (takes `Statement`) and 2.x (`execute_raw`).
pub(crate) async fn exec(
    conn: &impl ConnectionTrait,
    stmt: Statement,
) -> Result<ExecResult, sea_orm::DbErr> {
    #[cfg(feature = "sea-orm-2")]
    {
        conn.execute_raw(stmt).await
    }
    #[cfg(not(feature = "sea-orm-2"))]
    {
        conn.execute(stmt).await
    }
}

/// Query one row, abstracting over the API difference.
pub(crate) async fn query_one(
    conn: &impl ConnectionTrait,
    stmt: Statement,
) -> Result<Option<QueryResult>, sea_orm::DbErr> {
    #[cfg(feature = "sea-orm-2")]
    {
        conn.query_one_raw(stmt).await
    }
    #[cfg(not(feature = "sea-orm-2"))]
    {
        conn.query_one(stmt).await
    }
}
