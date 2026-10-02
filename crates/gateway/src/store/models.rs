//! The model catalog and who may call each model.

use std::collections::HashSet;

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};

use super::{write_error, Store, Tx, DEFAULT_ORG};

const MODEL_SELECT: &str = "SELECT m.id, m.provider_id, p.name AS provider_name, m.name,
            m.enabled, m.created_at
     FROM models m
     JOIN providers p ON p.id = m.provider_id AND p.org_id = m.org_id";

#[derive(Debug, Clone)]
pub struct ModelRow {
    pub id: i64,
    pub provider_id: i64,
    pub provider_name: String,
    pub name: String,
    pub enabled: bool,
    pub created_at: String,
}

fn model_from(r: &SqliteRow) -> ModelRow {
    ModelRow {
        id: r.get("id"),
        provider_id: r.get("provider_id"),
        provider_name: r.get("provider_name"),
        name: r.get("name"),
        enabled: r.get::<i64, _>("enabled") != 0,
        created_at: r.get("created_at"),
    }
}

/// One grant of a model. With neither team nor user it is for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantRow {
    pub model_id: i64,
    pub team_id: Option<i64>,
    pub user_id: Option<i64>,
}

/// Who may call a model, in the form the API shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    pub everyone: bool,
    pub team_ids: Vec<i64>,
    pub user_ids: Vec<i64>,
}

impl Tx<'_> {
    pub async fn model_by_id(&mut self, id: i64) -> Result<Option<ModelRow>> {
        let sql = format!("{MODEL_SELECT} WHERE m.id = ? AND m.org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(model_from))
    }

    /// The id of the provider's model of this name, if it has one.
    pub async fn model_id_by_name(&mut self, provider_id: i64, name: &str) -> Result<Option<i64>> {
        let id = sqlx::query_scalar(
            "SELECT id FROM models WHERE provider_id = ? AND name = ? AND org_id = ?",
        )
        .bind(provider_id)
        .bind(name)
        .bind(DEFAULT_ORG)
        .fetch_optional(self.conn())
        .await?;
        Ok(id)
    }

    /// The names the provider already has.
    pub async fn model_names_of(&mut self, provider_id: i64) -> Result<HashSet<String>> {
        let names: Vec<String> =
            sqlx::query_scalar("SELECT name FROM models WHERE provider_id = ? AND org_id = ?")
                .bind(provider_id)
                .bind(DEFAULT_ORG)
                .fetch_all(self.conn())
                .await?;
        Ok(names.into_iter().collect())
    }

    pub async fn count_models_of(&mut self, provider_id: i64) -> Result<i64> {
        let n =
            sqlx::query_scalar("SELECT COUNT(*) FROM models WHERE provider_id = ? AND org_id = ?")
                .bind(provider_id)
                .bind(DEFAULT_ORG)
                .fetch_one(self.conn())
                .await?;
        Ok(n)
    }

    /// A disabled model with nobody granted. A taken name is
    /// `StoreError::Duplicate`.
    pub async fn insert_model(&mut self, provider_id: i64, name: &str) -> Result<i64> {
        let r = sqlx::query("INSERT INTO models (org_id, provider_id, name) VALUES (?, ?, ?)")
            .bind(DEFAULT_ORG)
            .bind(provider_id)
            .bind(name)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.last_insert_rowid())
    }

    /// Returns `false` if there is no such model.
    pub async fn set_model_enabled(&mut self, id: i64, enabled: bool) -> Result<bool> {
        let r = sqlx::query("UPDATE models SET enabled = ? WHERE id = ? AND org_id = ?")
            .bind(enabled)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Returns `false` if there is no such model. Its grants go with it.
    pub async fn delete_model(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM models WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Replaces every grant of the model. The ids must be distinct and
    /// exist; the caller checks.
    pub async fn replace_grants(&mut self, model_id: i64, grants: &Grants) -> Result<()> {
        sqlx::query("DELETE FROM model_grants WHERE model_id = ? AND org_id = ?")
            .bind(model_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        let rows = grants
            .everyone
            .then_some((None, None))
            .into_iter()
            .chain(grants.team_ids.iter().map(|t| (Some(*t), None)))
            .chain(grants.user_ids.iter().map(|u| (None, Some(*u))));
        for (team_id, user_id) in rows {
            sqlx::query(
                "INSERT INTO model_grants (org_id, model_id, team_id, user_id) VALUES (?, ?, ?, ?)",
            )
            .bind(DEFAULT_ORG)
            .bind(model_id)
            .bind(team_id)
            .bind(user_id)
            .execute(self.conn())
            .await?;
        }
        Ok(())
    }

    pub async fn grants_of(&mut self, model_id: i64) -> Result<Grants> {
        let rows = sqlx::query(
            "SELECT model_id, team_id, user_id FROM model_grants
             WHERE model_id = ? AND org_id = ? ORDER BY id",
        )
        .bind(model_id)
        .bind(DEFAULT_ORG)
        .fetch_all(self.conn())
        .await?;
        Ok(grants_of_rows(rows.iter().map(grant_from)))
    }
}

fn grant_from(r: &SqliteRow) -> GrantRow {
    GrantRow {
        model_id: r.get("model_id"),
        team_id: r.get("team_id"),
        user_id: r.get("user_id"),
    }
}

/// The grants of one model, as the API shows them.
pub fn grants_of_rows(rows: impl Iterator<Item = GrantRow>) -> Grants {
    let mut grants = Grants::default();
    for row in rows {
        match (row.team_id, row.user_id) {
            (Some(team), _) => grants.team_ids.push(team),
            (None, Some(user)) => grants.user_ids.push(user),
            (None, None) => grants.everyone = true,
        }
    }
    grants
}

impl Store {
    /// Every model, ordered by provider name and then name.
    pub async fn list_models(&self) -> Result<Vec<ModelRow>> {
        let mut conn = self.pool().acquire().await?;
        list_models_in(&mut conn).await
    }

    pub async fn model_by_id(&self, id: i64) -> Result<Option<ModelRow>> {
        let sql = format!("{MODEL_SELECT} WHERE m.id = ? AND m.org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(model_from))
    }

    /// Every grant of every model, ordered by id.
    pub async fn list_model_grants(&self) -> Result<Vec<GrantRow>> {
        let mut conn = self.pool().acquire().await?;
        list_model_grants_in(&mut conn).await
    }

    pub async fn grants_of(&self, model_id: i64) -> Result<Grants> {
        let rows = sqlx::query(
            "SELECT model_id, team_id, user_id FROM model_grants
             WHERE model_id = ? AND org_id = ? ORDER BY id",
        )
        .bind(model_id)
        .bind(DEFAULT_ORG)
        .fetch_all(self.pool())
        .await?;
        Ok(grants_of_rows(rows.iter().map(grant_from)))
    }
}

pub(crate) async fn list_models_in(conn: &mut sqlx::SqliteConnection) -> Result<Vec<ModelRow>> {
    let sql = format!("{MODEL_SELECT} WHERE m.org_id = ? ORDER BY p.name, m.name");
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(model_from).collect())
}

pub(crate) async fn list_model_grants_in(
    conn: &mut sqlx::SqliteConnection,
) -> Result<Vec<GrantRow>> {
    let rows = sqlx::query(
        "SELECT model_id, team_id, user_id FROM model_grants WHERE org_id = ? ORDER BY id",
    )
    .bind(DEFAULT_ORG)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.iter().map(grant_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreError;

    #[tokio::test]
    async fn models_and_grants_round_trip() {
        let s = Store::open_in_memory().await.unwrap();
        let p = s
            .insert_provider("a", "openai", "http://a", None)
            .await
            .unwrap();
        let mut tx = s.begin().await.unwrap();
        let m = tx
            .insert_model(p, "meta-llama/Llama-3.3-70B")
            .await
            .unwrap();
        let dup = tx
            .insert_model(p, "meta-llama/Llama-3.3-70B")
            .await
            .unwrap_err();
        assert!(matches!(
            dup.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        let team = tx.insert_team("t").await.unwrap();
        tx.replace_grants(
            m,
            &Grants {
                everyone: true,
                team_ids: vec![team],
                user_ids: vec![],
            },
        )
        .await
        .unwrap();
        assert!(tx.set_model_enabled(m, true).await.unwrap());
        tx.commit().await.unwrap();

        let row = s.model_by_id(m).await.unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.provider_name, "a");
        let g = s.grants_of(m).await.unwrap();
        assert!(g.everyone);
        assert_eq!(g.team_ids, [team]);
        assert_eq!(s.list_model_grants().await.unwrap().len(), 2);

        // The foreign keys carry deletes along.
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_team(team).await.unwrap());
        tx.commit().await.unwrap();
        assert!(!s.grants_of(m).await.unwrap().team_ids.contains(&team));
    }

    #[tokio::test]
    async fn two_everyone_grants_are_refused_by_the_index() {
        let s = Store::open_in_memory().await.unwrap();
        let p = s
            .insert_provider("a", "openai", "http://a", None)
            .await
            .unwrap();
        let mut tx = s.begin().await.unwrap();
        let m = tx.insert_model(p, "m").await.unwrap();
        tx.commit().await.unwrap();
        let insert = || {
            sqlx::query("INSERT INTO model_grants (model_id) VALUES (?)")
                .bind(m)
                .execute(s.pool())
        };
        insert().await.unwrap();
        assert!(insert().await.is_err(), "a second grant for everyone");
    }
}
