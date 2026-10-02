//! Routes: named sets of model targets with fallbacks, limits and the
//! teams that may use them.

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use super::{write_error, Store, Tx, DEFAULT_ORG};
use crate::cache::{CacheScope, RouteCache};

/// The settings of a route, as stored and as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteSettings {
    pub retries: i64,
    pub first_token_timeout_ms: i64,
    pub total_timeout_ms: i64,
    pub breaker_failures: i64,
    pub breaker_window_s: i64,
    pub breaker_open_s: i64,
}

#[derive(Debug, Clone)]
pub struct RouteRow {
    pub id: i64,
    pub name: String,
    pub settings: RouteSettings,
    /// Every user may use the route. Otherwise only its teams, and admins.
    pub everyone: bool,
    pub cache: RouteCache,
    pub created_at: String,
}

/// One target of a route, with its model as the catalog shows it.
#[derive(Debug, Clone)]
pub struct TargetRow {
    pub route_id: i64,
    pub model_id: i64,
    pub provider_name: String,
    pub model_name: String,
    pub enabled: bool,
    pub primary: bool,
    pub weight: i64,
}

/// What `replace_targets` stores: primaries in order with their weights,
/// then fallbacks in order.
#[derive(Debug, Clone, Default)]
pub struct TargetsInput {
    pub primaries: Vec<(i64, i64)>,
    pub fallbacks: Vec<i64>,
}

fn route_from(r: &SqliteRow) -> RouteRow {
    RouteRow {
        id: r.get("id"),
        name: r.get("name"),
        settings: RouteSettings {
            retries: r.get("retries"),
            first_token_timeout_ms: r.get("first_token_timeout_ms"),
            total_timeout_ms: r.get("total_timeout_ms"),
            breaker_failures: r.get("breaker_failures"),
            breaker_window_s: r.get("breaker_window_s"),
            breaker_open_s: r.get("breaker_open_s"),
        },
        everyone: r.get::<i64, _>("everyone") != 0,
        cache: RouteCache {
            enabled: r.get::<i64, _>("cache_enabled") != 0,
            ttl_s: r.get("cache_ttl_s"),
            // The column is checked; a value that is not known is the default.
            scope: CacheScope::parse(&r.get::<String, _>("cache_scope"))
                .unwrap_or(CacheScope::Team),
        },
        created_at: r.get("created_at"),
    }
}

fn target_from(r: &SqliteRow) -> TargetRow {
    TargetRow {
        route_id: r.get("route_id"),
        model_id: r.get("model_id"),
        provider_name: r.get("provider_name"),
        model_name: r.get("model_name"),
        enabled: r.get::<i64, _>("enabled") != 0,
        primary: r.get::<String, _>("tier") == "primary",
        weight: r.get("weight"),
    }
}

const ROUTE_COLUMNS: &str = "id, name, retries, first_token_timeout_ms, total_timeout_ms,
            breaker_failures, breaker_window_s, breaker_open_s, everyone,
            cache_enabled, cache_ttl_s, cache_scope, created_at";

const TARGET_SELECT: &str = "SELECT t.route_id, t.model_id, t.tier, t.weight,
            p.name AS provider_name, m.name AS model_name, m.enabled
     FROM route_targets t
     JOIN models m ON m.id = t.model_id
     JOIN providers p ON p.id = m.provider_id";

/// Whether a write failed because a model or team it names no longer
/// exists.
pub fn is_missing_reference(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::Database(db)) if db.is_foreign_key_violation()
    )
}

impl Tx<'_> {
    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_route(
        &mut self,
        name: &str,
        s: &RouteSettings,
        everyone: bool,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO routes (org_id, name, retries, first_token_timeout_ms,
                total_timeout_ms, breaker_failures, breaker_window_s, breaker_open_s,
                everyone)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(name)
        .bind(s.retries)
        .bind(s.first_token_timeout_ms)
        .bind(s.total_timeout_ms)
        .bind(s.breaker_failures)
        .bind(s.breaker_window_s)
        .bind(s.breaker_open_s)
        .bind(everyone)
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.last_insert_rowid())
    }

    /// Returns `false` if there is no such route. A taken name is
    /// `StoreError::Duplicate`.
    pub async fn update_route(
        &mut self,
        id: i64,
        name: &str,
        s: &RouteSettings,
        everyone: bool,
    ) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE routes SET name = ?, retries = ?, first_token_timeout_ms = ?,
                total_timeout_ms = ?, breaker_failures = ?, breaker_window_s = ?,
                breaker_open_s = ?, everyone = ?
             WHERE id = ? AND org_id = ?",
        )
        .bind(name)
        .bind(s.retries)
        .bind(s.first_token_timeout_ms)
        .bind(s.total_timeout_ms)
        .bind(s.breaker_failures)
        .bind(s.breaker_window_s)
        .bind(s.breaker_open_s)
        .bind(everyone)
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    /// Sets the response cache of a route. Returns `false` if there is no
    /// such route.
    pub async fn set_route_cache(&mut self, id: i64, cache: &RouteCache) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE routes SET cache_enabled = ?, cache_ttl_s = ?, cache_scope = ?
             WHERE id = ? AND org_id = ?",
        )
        .bind(cache.enabled)
        .bind(cache.ttl_s)
        .bind(cache.scope.as_str())
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn route_by_id(&mut self, id: i64) -> Result<Option<RouteRow>> {
        let sql = format!("SELECT {ROUTE_COLUMNS} FROM routes WHERE id = ? AND org_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(route_from))
    }

    /// Replaces every target. The models must exist and be distinct; the
    /// caller checks.
    pub async fn replace_targets(&mut self, route_id: i64, t: &TargetsInput) -> Result<()> {
        sqlx::query("DELETE FROM route_targets WHERE route_id = ?")
            .bind(route_id)
            .execute(self.conn())
            .await?;
        let rows = t
            .primaries
            .iter()
            .map(|(m, w)| (*m, "primary", *w))
            .chain(t.fallbacks.iter().map(|m| (*m, "fallback", 1)));
        for (position, (model_id, tier, weight)) in rows.enumerate() {
            sqlx::query(
                "INSERT INTO route_targets (route_id, model_id, tier, weight, position)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(route_id)
            .bind(model_id)
            .bind(tier)
            .bind(weight)
            .bind(i64::try_from(position)?)
            .execute(self.conn())
            .await?;
        }
        Ok(())
    }

    /// Replaces the teams granted. The ids must be distinct and exist.
    pub async fn replace_route_grants(&mut self, route_id: i64, team_ids: &[i64]) -> Result<()> {
        sqlx::query("DELETE FROM route_grants WHERE route_id = ?")
            .bind(route_id)
            .execute(self.conn())
            .await?;
        for team_id in team_ids {
            sqlx::query("INSERT INTO route_grants (route_id, team_id) VALUES (?, ?)")
                .bind(route_id)
                .bind(team_id)
                .execute(self.conn())
                .await?;
        }
        Ok(())
    }

    /// Returns `false` if there is no such route. Targets and grants go
    /// with it.
    pub async fn delete_route(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM routes WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }
}

impl Store {
    /// Every route, ordered by name.
    pub async fn list_routes(&self) -> Result<Vec<RouteRow>> {
        let mut conn = self.pool().acquire().await?;
        list_routes_in(&mut conn).await
    }

    pub async fn route_by_id(&self, id: i64) -> Result<Option<RouteRow>> {
        let sql = format!("SELECT {ROUTE_COLUMNS} FROM routes WHERE id = ? AND org_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(route_from))
    }

    /// Every target of every route, primaries before fallbacks, each in
    /// its stored order.
    pub async fn list_route_targets(&self) -> Result<Vec<TargetRow>> {
        let mut conn = self.pool().acquire().await?;
        list_route_targets_in(&mut conn).await
    }

    pub async fn route_targets_of(&self, route_id: i64) -> Result<Vec<TargetRow>> {
        let sql = format!("{TARGET_SELECT} WHERE t.route_id = ? ORDER BY t.position");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(route_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(target_from).collect())
    }

    /// `(route_id, team_id)` for every grant.
    pub async fn list_route_grants(&self) -> Result<Vec<(i64, i64)>> {
        let mut conn = self.pool().acquire().await?;
        list_route_grants_in(&mut conn).await
    }

    pub async fn route_team_ids(&self, route_id: i64) -> Result<Vec<i64>> {
        Ok(
            sqlx::query_scalar(
                "SELECT team_id FROM route_grants WHERE route_id = ? ORDER BY rowid",
            )
            .bind(route_id)
            .fetch_all(self.pool())
            .await?,
        )
    }
}

pub(crate) async fn list_routes_in(conn: &mut sqlx::SqliteConnection) -> Result<Vec<RouteRow>> {
    let sql = format!("SELECT {ROUTE_COLUMNS} FROM routes WHERE org_id = ? ORDER BY name");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(route_from).collect())
}

pub(crate) async fn list_route_targets_in(
    conn: &mut sqlx::SqliteConnection,
) -> Result<Vec<TargetRow>> {
    let sql = format!("{TARGET_SELECT} ORDER BY t.route_id, t.position");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(target_from).collect())
}

pub(crate) async fn list_route_grants_in(
    conn: &mut sqlx::SqliteConnection,
) -> Result<Vec<(i64, i64)>> {
    let rows = sqlx::query("SELECT route_id, team_id FROM route_grants ORDER BY rowid")
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("route_id"), r.get("team_id")))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreError;

    const DEFAULTS: RouteSettings = RouteSettings {
        retries: 2,
        first_token_timeout_ms: 30_000,
        total_timeout_ms: 300_000,
        breaker_failures: 5,
        breaker_window_s: 60,
        breaker_open_s: 30,
    };

    #[tokio::test]
    async fn migration_closes_routes_that_had_grants() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let team = tx.insert_team("t").await.unwrap();
        let open = tx.insert_route("open", &DEFAULTS, true).await.unwrap();
        let granted = tx.insert_route("granted", &DEFAULTS, true).await.unwrap();
        tx.replace_route_grants(granted, &[team]).await.unwrap();
        tx.commit().await.unwrap();
        // The data statement of the migration, on rows as 0004 left them.
        let sql = include_str!("../../migrations/0005_route_everyone.sql");
        let update = sql
            .split(';')
            .find(|st| st.contains("UPDATE routes"))
            .expect("the migration updates routes");
        sqlx::query(sqlx::AssertSqlSafe(update.to_string()))
            .execute(s.pool())
            .await
            .unwrap();
        assert!(s.route_by_id(open).await.unwrap().unwrap().everyone);
        assert!(!s.route_by_id(granted).await.unwrap().unwrap().everyone);
    }

    #[tokio::test]
    async fn a_missing_model_or_team_is_a_foreign_key_failure() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let r = tx.insert_route("r", &DEFAULTS, true).await.unwrap();
        let e = tx
            .replace_targets(
                r,
                &TargetsInput {
                    primaries: vec![(999, 1)],
                    fallbacks: vec![],
                },
            )
            .await
            .unwrap_err();
        assert!(is_missing_reference(&e));
        let e = tx.replace_route_grants(r, &[999]).await.unwrap_err();
        assert!(is_missing_reference(&e));
    }

    #[tokio::test]
    async fn routes_round_trip_and_cascade() {
        let s = Store::open_in_memory().await.unwrap();
        let p = s
            .insert_provider("a", "openai", "http://a", None)
            .await
            .unwrap();
        let mut tx = s.begin().await.unwrap();
        let m1 = tx.insert_model(p, "m1").await.unwrap();
        let m2 = tx.insert_model(p, "m2").await.unwrap();
        let team = tx.insert_team("t").await.unwrap();
        let r = tx.insert_route("r", &DEFAULTS, true).await.unwrap();
        let dup = tx.insert_route("r", &DEFAULTS, true).await.unwrap_err();
        assert!(matches!(
            dup.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        tx.replace_targets(
            r,
            &TargetsInput {
                primaries: vec![(m1, 4)],
                fallbacks: vec![m2],
            },
        )
        .await
        .unwrap();
        tx.replace_route_grants(r, &[team]).await.unwrap();
        tx.commit().await.unwrap();

        let t = s.route_targets_of(r).await.unwrap();
        assert_eq!(t.len(), 2);
        assert!(t[0].primary && t[0].weight == 4 && t[0].model_name == "m1");
        assert!(!t[1].primary && t[1].model_name == "m2");
        assert_eq!(s.route_team_ids(r).await.unwrap(), [team]);

        // The foreign keys carry deletes along.
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_model(m1).await.unwrap());
        assert!(tx.delete_team(team).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(s.route_targets_of(r).await.unwrap().len(), 1);
        assert!(s.route_team_ids(r).await.unwrap().is_empty());
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_route(r).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.list_route_targets().await.unwrap().is_empty());
    }
}
