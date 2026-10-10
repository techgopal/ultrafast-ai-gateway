//! Prompt templates and their versions. A version is written once and never
//! changes (the database refuses an update of its text); a new text is a new
//! version, numbered from 1 in the order they are written. The trigger
//! guards UPDATE only: a raw `DELETE` of a single version is not blocked
//! (the API never deletes one; only a whole template goes, with its
//! versions), so "no gaps" is kept by the code, not enforced by the database.

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::{now, write_error, Store, Tx, DEFAULT_ORG};

const TEMPLATE: &str = "SELECT id, name, description, created_by, created_at FROM prompt_templates";

const VERSION: &str = "SELECT v.id, v.template_id, v.version, v.messages, v.variables, v.model,
        v.params, v.created_by, v.created_at
     FROM prompt_versions v JOIN prompt_templates t ON t.id = v.template_id";

/// A template as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRow {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// The user who made it; `None` when they are gone (or an import from the
    /// command line made it): only admins manage it then.
    pub created_by: Option<i64>,
    pub created_at: String,
}

/// One version of a template as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub id: i64,
    pub template_id: i64,
    /// From 1, without gaps.
    pub version: i64,
    /// A JSON array of `{role, content}`.
    pub messages: String,
    /// A JSON array of the variable names the messages use.
    pub variables: String,
    pub model: Option<String>,
    /// A JSON object: `temperature`, `max_tokens`, `top_p`, `response_format`.
    pub params: String,
    pub created_by: Option<i64>,
    pub created_at: String,
}

/// What `insert_version` stores.
pub struct NewVersion<'a> {
    pub messages: &'a str,
    pub variables: &'a str,
    pub model: Option<&'a str>,
    pub params: &'a str,
}

fn template_from(r: &AnyRow) -> TemplateRow {
    TemplateRow {
        id: r.get("id"),
        name: r.get("name"),
        description: r.get("description"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
    }
}

fn version_from(r: &AnyRow) -> VersionRow {
    VersionRow {
        id: r.get("id"),
        template_id: r.get("template_id"),
        version: r.get("version"),
        messages: r.get("messages"),
        variables: r.get("variables"),
        model: r.get("model"),
        params: r.get("params"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
    }
}

/// Every template by name, on any connection.
pub(super) async fn list_templates_in(conn: &mut AnyConnection) -> Result<Vec<TemplateRow>> {
    let sql = format!("{TEMPLATE} WHERE org_id = ? ORDER BY name");
    let rows = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(template_from).collect())
}

/// Every version of every template, by template, then version.
pub(super) async fn list_versions_in(conn: &mut AnyConnection) -> Result<Vec<VersionRow>> {
    let sql = format!("{VERSION} WHERE t.org_id = ? ORDER BY v.template_id, v.version");
    let rows = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(version_from).collect())
}

/// The newest version of every template, by template.
pub(super) async fn list_latest_versions_in(conn: &mut AnyConnection) -> Result<Vec<VersionRow>> {
    let sql = format!(
        "{VERSION} WHERE t.org_id = ? AND v.version =
           (SELECT MAX(version) FROM prompt_versions WHERE template_id = v.template_id)
         ORDER BY v.template_id"
    );
    let rows = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(version_from).collect())
}

/// `(template id, number of its newest version)` of every template that has
/// a version.
pub(super) async fn latest_numbers_in(conn: &mut AnyConnection) -> Result<Vec<(i64, i64)>> {
    let rows = conn
        .q("SELECT v.template_id, MAX(v.version) FROM prompt_versions v
            JOIN prompt_templates t ON t.id = v.template_id
            WHERE t.org_id = ? GROUP BY v.template_id ORDER BY v.template_id")
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
}

/// How many templates are read one by one before one read of them all is
/// cheaper.
const READ_ONE_BY_ONE: usize = 25;

/// The newest versions, with their text, of the templates `known` does not
/// already hold: a refresh reads the text of what changed, not of every
/// template. `latest` is [`latest_numbers_in`].
pub(super) async fn changed_latest_versions_in(
    conn: &mut AnyConnection,
    templates: &[TemplateRow],
    latest: &[(i64, i64)],
    known: &(dyn Fn(&TemplateRow, i64) -> bool + Send + Sync),
) -> Result<Vec<VersionRow>> {
    let needed: Vec<(i64, i64)> = templates
        .iter()
        .filter_map(|t| {
            let number = latest.iter().find(|(id, _)| *id == t.id)?.1;
            (!known(t, number)).then_some((t.id, number))
        })
        .collect();
    if needed.len() > READ_ONE_BY_ONE {
        let all = list_latest_versions_in(conn).await?;
        return Ok(all
            .into_iter()
            .filter(|v| needed.iter().any(|(id, _)| *id == v.template_id))
            .collect());
    }
    let mut out = Vec::with_capacity(needed.len());
    for (id, number) in needed {
        let sql = format!("{VERSION} WHERE v.template_id = ? AND v.version = ? AND t.org_id = ?");
        if let Some(row) = conn
            .q_dyn(sql)
            .bind(id)
            .bind(number)
            .bind(DEFAULT_ORG)
            .fetch_optional(&mut *conn)
            .await?
        {
            out.push(version_from(&row));
        }
    }
    Ok(out)
}

/// The newest version of a template without its text: what a list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestStub {
    pub template_id: i64,
    pub version: i64,
    pub created_at: String,
    pub model: Option<String>,
    /// A JSON array of the variable names.
    pub variables: String,
}

/// A version without its text: what a list of versions shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionStub {
    pub version: i64,
    pub created_by: Option<i64>,
    pub created_at: String,
}

impl Tx<'_> {
    /// How many templates there are.
    pub async fn count_prompt_templates(&mut self) -> Result<i64> {
        let n: i64 = self
            .scalar("SELECT COUNT(*) FROM prompt_templates WHERE org_id = ?")
            .bind(DEFAULT_ORG)
            .fetch_one(self.conn())
            .await?;
        Ok(n)
    }

    /// How many templates this user made.
    pub async fn count_prompt_templates_by(&mut self, user_id: i64) -> Result<i64> {
        let n: i64 = self
            .scalar("SELECT COUNT(*) FROM prompt_templates WHERE org_id = ? AND created_by = ?")
            .bind(DEFAULT_ORG)
            .bind(user_id)
            .fetch_one(self.conn())
            .await?;
        Ok(n)
    }

    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_prompt_template(
        &mut self,
        name: &str,
        description: &str,
        created_by: Option<i64>,
    ) -> Result<i64> {
        let id: i64 = self
            .scalar(
                "INSERT INTO prompt_templates (org_id, name, description, created_by, created_at)
                 VALUES (?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(name)
            .bind(description)
            .bind(created_by)
            .bind(now())
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// For a read inside a transaction; see `Store::prompt_template`.
    pub async fn prompt_template(&mut self, id: i64) -> Result<Option<TemplateRow>> {
        let sql = format!("{TEMPLATE} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(template_from))
    }

    pub async fn set_prompt_description(&mut self, id: i64, description: &str) -> Result<bool> {
        let r = self
            .q("UPDATE prompt_templates SET description = ? WHERE id = ? AND org_id = ?")
            .bind(description)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Its versions go with it. `false`: no such template.
    pub async fn delete_prompt_template(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM prompt_templates WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// The highest version of a template; 0 when it has none.
    pub async fn latest_prompt_version(&mut self, template_id: i64) -> Result<i64> {
        let n: i64 = self
            .scalar("SELECT COALESCE(MAX(version), 0) FROM prompt_versions WHERE template_id = ?")
            .bind(template_id)
            .fetch_one(self.conn())
            .await?;
        Ok(n)
    }

    /// Writes the next version of a template and returns its number. The
    /// number is read and written here, so call it inside
    /// `Store::begin_immediate`; the unique index on (template, version) is
    /// the last guard.
    pub async fn insert_prompt_version(
        &mut self,
        template_id: i64,
        v: NewVersion<'_>,
        created_by: Option<i64>,
    ) -> Result<i64> {
        let version = self.latest_prompt_version(template_id).await? + 1;
        self.q("INSERT INTO prompt_versions
             (template_id, version, messages, variables, model, params, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(template_id)
            .bind(version)
            .bind(v.messages)
            .bind(v.variables)
            .bind(v.model)
            .bind(v.params)
            .bind(created_by)
            .bind(now())
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(version)
    }

    /// The versions of a template, oldest first.
    pub async fn prompt_versions(&mut self, template_id: i64) -> Result<Vec<VersionRow>> {
        let sql = format!("{VERSION} WHERE v.template_id = ? AND t.org_id = ? ORDER BY v.version");
        let rows = self
            .q_dyn(sql)
            .bind(template_id)
            .bind(DEFAULT_ORG)
            .fetch_all(self.conn())
            .await?;
        Ok(rows.iter().map(version_from).collect())
    }
}

impl Store {
    /// The newest version of every template (the only text a list reads).
    pub async fn list_latest_prompt_versions(&self) -> Result<Vec<VersionRow>> {
        let mut conn = self.pool().acquire().await?;
        list_latest_versions_in(&mut conn).await
    }

    /// The newest version of every template without its messages: a list of
    /// templates must not read and parse every text.
    pub async fn list_latest_prompt_stubs(&self) -> Result<Vec<LatestStub>> {
        let rows = self
            .q(
                "SELECT v.template_id, v.version, v.created_at, v.model, v.variables
                 FROM prompt_versions v JOIN prompt_templates t ON t.id = v.template_id
                 WHERE t.org_id = ? AND v.version =
                   (SELECT MAX(version) FROM prompt_versions WHERE template_id = v.template_id)
                 ORDER BY v.template_id",
            )
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|r| LatestStub {
                template_id: r.get("template_id"),
                version: r.get("version"),
                created_at: r.get("created_at"),
                model: r.get("model"),
                variables: r.get("variables"),
            })
            .collect())
    }

    /// How many templates this user made.
    pub async fn count_prompt_templates_by(&self, user_id: i64) -> Result<i64> {
        let n: i64 = self
            .scalar("SELECT COUNT(*) FROM prompt_templates WHERE org_id = ? AND created_by = ?")
            .bind(DEFAULT_ORG)
            .bind(user_id)
            .fetch_one(self.pool())
            .await?;
        Ok(n)
    }

    /// `(template id, number of versions)` of every template.
    pub async fn prompt_version_counts(&self) -> Result<Vec<(i64, i64)>> {
        let rows = self
            .q("SELECT template_id, COUNT(*) FROM prompt_versions GROUP BY template_id")
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// The newest version of a template.
    pub async fn prompt_latest_version(&self, template_id: i64) -> Result<Option<VersionRow>> {
        let sql = format!(
            "{VERSION} WHERE v.template_id = ? AND t.org_id = ?
             ORDER BY v.version DESC LIMIT 1"
        );
        let row = self
            .q_dyn(sql)
            .bind(template_id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(version_from))
    }

    /// The versions of a template without their text, oldest first.
    pub async fn prompt_version_stubs(&self, template_id: i64) -> Result<Vec<VersionStub>> {
        let rows = self
            .q("SELECT version, created_by, created_at FROM prompt_versions
                WHERE template_id = ? ORDER BY version")
            .bind(template_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|r| VersionStub {
                version: r.get(0),
                created_by: r.get(1),
                created_at: r.get(2),
            })
            .collect())
    }

    /// Every template, by name.
    pub async fn list_prompt_templates(&self) -> Result<Vec<TemplateRow>> {
        let mut conn = self.pool().acquire().await?;
        list_templates_in(&mut conn).await
    }

    /// Every version of every template, by template, then version.
    pub async fn list_prompt_versions(&self) -> Result<Vec<VersionRow>> {
        let mut conn = self.pool().acquire().await?;
        list_versions_in(&mut conn).await
    }

    pub async fn prompt_template(&self, id: i64) -> Result<Option<TemplateRow>> {
        let sql = format!("{TEMPLATE} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(template_from))
    }

    /// The versions of a template, oldest first.
    pub async fn prompt_versions(&self, template_id: i64) -> Result<Vec<VersionRow>> {
        let sql = format!("{VERSION} WHERE v.template_id = ? AND t.org_id = ? ORDER BY v.version");
        let rows = self
            .q_dyn(sql)
            .bind(template_id)
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(version_from).collect())
    }

    pub async fn prompt_version(
        &self,
        template_id: i64,
        version: i64,
    ) -> Result<Option<VersionRow>> {
        let sql = format!("{VERSION} WHERE v.template_id = ? AND v.version = ? AND t.org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(template_id)
            .bind(version)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(version_from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreError;

    fn version<'a>(text: &'a str) -> NewVersion<'a> {
        NewVersion {
            messages: text,
            variables: "[]",
            model: None,
            params: "{}",
        }
    }

    #[tokio::test]
    async fn versions_are_numbered_from_one_and_names_are_unique() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin_immediate().await.unwrap();
        let t = tx.insert_prompt_template("greet", "d", None).await.unwrap();
        assert_eq!(tx.latest_prompt_version(t).await.unwrap(), 0);
        for (i, text) in ["[1]", "[2]", "[3]"].into_iter().enumerate() {
            assert_eq!(
                tx.insert_prompt_version(t, version(text), None)
                    .await
                    .unwrap(),
                i as i64 + 1
            );
        }
        tx.commit().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let dup = tx
            .insert_prompt_template("greet", "", None)
            .await
            .unwrap_err();
        assert!(matches!(
            dup.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);
        let all = s.prompt_versions(t).await.unwrap();
        assert_eq!(
            all.iter()
                .map(|v| (v.version, v.messages.as_str()))
                .collect::<Vec<_>>(),
            [(1, "[1]"), (2, "[2]"), (3, "[3]")]
        );
        assert_eq!(
            s.prompt_version(t, 2).await.unwrap().unwrap().messages,
            "[2]"
        );
        assert!(s.prompt_version(t, 4).await.unwrap().is_none());
        assert_eq!(s.list_prompt_versions().await.unwrap().len(), 3);
    }

    /// The database itself refuses to change the text of a version, not only
    /// the API: a stray statement cannot rewrite what a call was rendered from.
    #[tokio::test]
    async fn the_database_refuses_to_change_a_version() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin_immediate().await.unwrap();
        let t = tx.insert_prompt_template("greet", "", None).await.unwrap();
        tx.insert_prompt_version(t, version("[\"one\"]"), None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        for sql in [
            "UPDATE prompt_versions SET messages = '[]'",
            "UPDATE prompt_versions SET variables = '[\"x\"]'",
            "UPDATE prompt_versions SET model = 'm'",
            "UPDATE prompt_versions SET params = '{\"temperature\":1}'",
            "UPDATE prompt_versions SET version = 9",
            "UPDATE prompt_versions SET template_id = template_id + 1",
        ] {
            let e = s.q(sql).execute(s.pool()).await.unwrap_err();
            assert!(e.to_string().contains("never change"), "{sql}: {e}");
        }
        assert_eq!(
            s.prompt_version(t, 1).await.unwrap().unwrap().messages,
            "[\"one\"]"
        );
        // What may change is who made it: the user going away leaves it unowned.
        let mut tx = s.begin().await.unwrap();
        let user = tx
            .insert_user(crate::store::NewUser {
                email: "a@example.com",
                name: "A",
                role: crate::identity::Role::Member,
                status: crate::identity::UserStatus::Active,
                password_hash: None,
            })
            .await
            .unwrap();
        let t2 = tx
            .insert_prompt_template("owned", "", Some(user))
            .await
            .unwrap();
        tx.insert_prompt_version(t2, version("[]"), Some(user))
            .await
            .unwrap();
        assert!(tx.delete_user(user).await.unwrap());
        tx.commit().await.unwrap();
        let row = s.prompt_template(t2).await.unwrap().unwrap();
        assert_eq!(row.created_by, None);
        assert_eq!(s.prompt_versions(t2).await.unwrap()[0].created_by, None);
        // A template takes its versions with it when it goes.
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_prompt_template(t).await.unwrap());
        assert!(!tx.delete_prompt_template(t).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(s.list_prompt_versions().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_request_log_has_a_column_for_the_prompt() {
        let s = Store::open_in_memory().await.unwrap();
        let rows = s
            .q("SELECT prompt FROM request_logs")
            .fetch_all(s.pool())
            .await
            .unwrap();
        assert!(rows.is_empty());
    }
}
