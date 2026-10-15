//! MCP servers the gateway proxies, the tools they offer and who may call
//! each tool. A server's secret arrives already encrypted (the caller holds
//! the cipher); this module never sees plaintext. A sync marks a tool it no
//! longer sees as removed instead of deleting it, so its grants are kept.

use std::collections::HashMap;
use std::fmt;

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::models::{grants_of_rows, GrantRow, Grants};
use super::{flag, now, write_error, Store, Tx, DEFAULT_ORG};

const SERVER_SELECT: &str = "SELECT id, name, url, auth_kind, auth_header, auth_secret_enc,
        enabled, description, created_at, updated_at, last_sync_at, last_sync_error,
        transport, command, args, env_enc, catalog_id
     FROM mcp_servers";

const TOOL_SELECT: &str = "SELECT id, server_id, name, title, description, input_schema,
        output_schema, annotations, enabled, removed_at
     FROM mcp_tools";

/// An MCP server as stored.
#[derive(Clone)]
pub struct McpServerRow {
    pub id: i64,
    pub name: String,
    pub url: String,
    /// `none`, `bearer` or `header`.
    pub auth_kind: String,
    /// The header name when `auth_kind` is `header`.
    pub auth_header: Option<String>,
    /// The secret, encrypted by the caller.
    pub auth_secret_enc: Option<String>,
    pub enabled: bool,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_sync_at: Option<String>,
    /// Our own summary of why the last sync failed, never upstream text.
    pub last_sync_error: Option<String>,
    /// `http`, or `stdio` for a program the gateway runs. A stdio server has
    /// an empty `url`.
    pub transport: String,
    /// The program of a stdio server, run with `args` and no shell.
    pub command: Option<String>,
    pub args: Vec<String>,
    /// The environment values of a stdio server, each encrypted by the caller.
    pub env_enc: std::collections::BTreeMap<String, String>,
    /// The catalog entry the server was made from.
    pub catalog_id: Option<String>,
}

impl McpServerRow {
    pub fn is_local(&self) -> bool {
        self.transport == "stdio"
    }
}

/// Shows no URL and no secret, not even encrypted.
impl fmt::Debug for McpServerRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpServerRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("auth_kind", &self.auth_kind)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// What a server is written with. `auth_secret_enc` is already encrypted.
/// The fields after `description` are for a stdio server; `..Default::default()`
/// gives an HTTP one.
#[derive(Clone)]
pub struct NewMcpServer<'a> {
    pub name: &'a str,
    pub url: &'a str,
    pub auth_kind: &'a str,
    pub auth_header: Option<&'a str>,
    pub auth_secret_enc: Option<&'a str>,
    pub enabled: bool,
    pub description: &'a str,
    /// `http` or `stdio`.
    pub transport: &'a str,
    pub command: Option<&'a str>,
    pub args: &'a [String],
    /// Name to encrypted value.
    pub env_enc: &'a std::collections::BTreeMap<String, String>,
    pub catalog_id: Option<&'a str>,
}

static NO_ENV: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();

impl Default for NewMcpServer<'_> {
    fn default() -> Self {
        Self {
            name: "",
            url: "",
            auth_kind: "none",
            auth_header: None,
            auth_secret_enc: None,
            enabled: true,
            description: "",
            transport: "http",
            command: None,
            args: &[],
            env_enc: &NO_ENV,
            catalog_id: None,
        }
    }
}

/// A tool as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolRow {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    /// JSON as the upstream gave it.
    pub input_schema: String,
    pub output_schema: Option<String>,
    pub annotations: Option<String>,
    pub enabled: bool,
    /// Set when a sync no longer saw the tool.
    pub removed_at: Option<String>,
}

/// What of a tool's result the agent is given (see `mcp::project`). The
/// queries are JSONPath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRow {
    pub tool_id: i64,
    pub keep: Vec<String>,
    pub drop: Vec<String>,
    pub max_items: Option<i64>,
    pub max_bytes: Option<i64>,
}

fn projection_from(r: &AnyRow) -> ProjectionRow {
    ProjectionRow {
        tool_id: r.get("tool_id"),
        keep: serde_json::from_str(&r.get::<String, _>("keep_paths")).unwrap_or_default(),
        drop: serde_json::from_str(&r.get::<String, _>("drop_paths")).unwrap_or_default(),
        max_items: r.get("max_items"),
        max_bytes: r.get("max_bytes"),
    }
}

const PROJECTION_SELECT: &str =
    "SELECT tool_id, keep_paths, drop_paths, max_items, max_bytes FROM mcp_tool_projections";

/// A tool as a sync found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTool {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: String,
    pub output_schema: Option<String>,
    pub annotations: Option<String>,
}

/// What a sync changed, by tool name. `updated` includes a removed tool that
/// came back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncDelta {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

/// One grant of a tool. With neither team nor user it is for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpGrantRow {
    pub tool_id: i64,
    pub team_id: Option<i64>,
    pub user_id: Option<i64>,
}

fn server_from(r: &AnyRow) -> McpServerRow {
    McpServerRow {
        id: r.get("id"),
        name: r.get("name"),
        url: r.get("url"),
        auth_kind: r.get("auth_kind"),
        auth_header: r.get("auth_header"),
        auth_secret_enc: r.get("auth_secret_enc"),
        enabled: r.get::<i64, _>("enabled") != 0,
        description: r.get("description"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        last_sync_at: r.get("last_sync_at"),
        last_sync_error: r.get("last_sync_error"),
        transport: r.get("transport"),
        command: r.get("command"),
        args: serde_json::from_str(&r.get::<String, _>("args")).unwrap_or_default(),
        env_enc: serde_json::from_str(&r.get::<String, _>("env_enc")).unwrap_or_default(),
        catalog_id: r.get("catalog_id"),
    }
}

fn tool_from(r: &AnyRow) -> McpToolRow {
    McpToolRow {
        id: r.get("id"),
        server_id: r.get("server_id"),
        name: r.get("name"),
        title: r.get("title"),
        description: r.get("description"),
        input_schema: r.get("input_schema"),
        output_schema: r.get("output_schema"),
        annotations: r.get("annotations"),
        enabled: r.get::<i64, _>("enabled") != 0,
        removed_at: r.get("removed_at"),
    }
}

fn grant_from(r: &AnyRow) -> McpGrantRow {
    McpGrantRow {
        tool_id: r.get("tool_id"),
        team_id: r.get("team_id"),
        user_id: r.get("user_id"),
    }
}

fn same(row: &McpToolRow, t: &DiscoveredTool) -> bool {
    row.title == t.title
        && row.description == t.description
        && row.input_schema == t.input_schema
        && row.output_schema == t.output_schema
        && row.annotations == t.annotations
}

impl Tx<'_> {
    /// Sets the projection of a tool, replacing the old one.
    pub async fn put_projection(&mut self, p: &ProjectionRow) -> Result<()> {
        self.q(
            "INSERT INTO mcp_tool_projections (tool_id, keep_paths, drop_paths, max_items, max_bytes)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (tool_id) DO UPDATE SET keep_paths = excluded.keep_paths,
                drop_paths = excluded.drop_paths, max_items = excluded.max_items,
                max_bytes = excluded.max_bytes",
        )
        .bind(p.tool_id)
        .bind(serde_json::to_string(&p.keep)?)
        .bind(serde_json::to_string(&p.drop)?)
        .bind(p.max_items)
        .bind(p.max_bytes)
        .execute(self.conn())
        .await?;
        Ok(())
    }

    /// Takes the projection off a tool. Returns whether it had one.
    pub async fn delete_projection(&mut self, tool_id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM mcp_tool_projections WHERE tool_id = ?")
            .bind(tool_id)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// For use inside a transaction; see `Store::server_by_id`.
    pub async fn server_by_id(&mut self, id: i64) -> Result<Option<McpServerRow>> {
        let sql = format!("{SERVER_SELECT} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(server_from))
    }

    /// For use inside a transaction; see `Store::tool_by_id`.
    pub async fn tool_by_id(&mut self, id: i64) -> Result<Option<McpToolRow>> {
        let sql = format!("{TOOL_SELECT} WHERE id = ?");
        let row = self.q_dyn(sql).bind(id).fetch_optional(self.conn()).await?;
        Ok(row.as_ref().map(tool_from))
    }

    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_server(&mut self, s: NewMcpServer<'_>) -> Result<i64> {
        let at = now();
        let id: i64 = self
            .scalar(
                "INSERT INTO mcp_servers (org_id, name, url, auth_kind, auth_header,
                    auth_secret_enc, enabled, description, created_at, updated_at,
                    transport, command, args, env_enc, catalog_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(s.name)
            .bind(s.url)
            .bind(s.auth_kind)
            .bind(s.auth_header)
            .bind(s.auth_secret_enc)
            .bind(flag(s.enabled))
            .bind(s.description)
            .bind(at.clone())
            .bind(at)
            .bind(s.transport)
            .bind(s.command)
            .bind(serde_json::to_string(s.args)?)
            .bind(serde_json::to_string(s.env_enc)?)
            .bind(s.catalog_id)
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// Replaces every field the caller can set (the catalog entry a server
    /// was made from is kept). Returns `false` if there is no such server; a taken name is `StoreError::Duplicate`.
    pub async fn update_server(&mut self, id: i64, s: NewMcpServer<'_>) -> Result<bool> {
        let r = self
            .q(
                "UPDATE mcp_servers SET name = ?, url = ?, auth_kind = ?, auth_header = ?,
                    auth_secret_enc = ?, enabled = ?, description = ?, updated_at = ?,
                    transport = ?, command = ?, args = ?, env_enc = ?
                 WHERE id = ? AND org_id = ?",
            )
            .bind(s.name)
            .bind(s.url)
            .bind(s.auth_kind)
            .bind(s.auth_header)
            .bind(s.auth_secret_enc)
            .bind(flag(s.enabled))
            .bind(s.description)
            .bind(now())
            .bind(s.transport)
            .bind(s.command)
            .bind(serde_json::to_string(s.args)?)
            .bind(serde_json::to_string(s.env_enc)?)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    /// Returns `false` if there is no such server. Its tools and their
    /// grants go with it.
    pub async fn delete_server(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM mcp_servers WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Records a sync: when it ran, and our summary of why it failed (`None`
    /// clears it). Returns `false` if there is no such server.
    pub async fn record_sync(&mut self, id: i64, error: Option<&str>) -> Result<bool> {
        let r = self
            .q(
                "UPDATE mcp_servers SET last_sync_at = ?, last_sync_error = ?
                 WHERE id = ? AND org_id = ?",
            )
            .bind(now())
            .bind(error)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Brings the server's tools in line with what a sync found: new ones are
    /// added switched off, changed ones updated (the switch and the grants
    /// stay), ones not seen are marked removed, and ones seen again are
    /// cleared. A name listed twice counts once, the last wins.
    pub async fn upsert_tools(
        &mut self,
        server_id: i64,
        found: Vec<DiscoveredTool>,
    ) -> Result<SyncDelta> {
        let sql = format!("{TOOL_SELECT} WHERE server_id = ? ORDER BY name");
        let rows = self
            .q_dyn(sql)
            .bind(server_id)
            .fetch_all(self.conn())
            .await?;
        let mut have: HashMap<String, McpToolRow> = rows
            .iter()
            .map(tool_from)
            .map(|t| (t.name.clone(), t))
            .collect();
        let mut seen: HashMap<String, DiscoveredTool> = HashMap::new();
        let mut order = Vec::new();
        for t in found {
            let name = t.name.clone();
            if seen.insert(name.clone(), t).is_none() {
                order.push(name);
            }
        }
        let mut delta = SyncDelta::default();
        for name in &order {
            let t = &seen[name];
            match have.remove(name) {
                None => {
                    self.q("INSERT INTO mcp_tools (server_id, name, title, description,
                            input_schema, output_schema, annotations)
                         VALUES (?, ?, ?, ?, ?, ?, ?)")
                        .bind(server_id)
                        .bind(&t.name)
                        .bind(t.title.as_deref())
                        .bind(&t.description)
                        .bind(&t.input_schema)
                        .bind(t.output_schema.as_deref())
                        .bind(t.annotations.as_deref())
                        .execute(self.conn())
                        .await?;
                    delta.added.push(name.clone());
                }
                Some(row) if row.removed_at.is_none() && same(&row, t) => {}
                Some(row) => {
                    self.q(
                        "UPDATE mcp_tools SET title = ?, description = ?, input_schema = ?,
                            output_schema = ?, annotations = ?, removed_at = NULL
                         WHERE id = ?",
                    )
                    .bind(t.title.as_deref())
                    .bind(&t.description)
                    .bind(&t.input_schema)
                    .bind(t.output_schema.as_deref())
                    .bind(t.annotations.as_deref())
                    .bind(row.id)
                    .execute(self.conn())
                    .await?;
                    delta.updated.push(name.clone());
                }
            }
        }
        let mut gone: Vec<McpToolRow> = have
            .into_values()
            .filter(|r| r.removed_at.is_none())
            .collect();
        gone.sort_by(|a, b| a.name.cmp(&b.name));
        let at = now();
        for row in gone {
            self.q("UPDATE mcp_tools SET removed_at = ? WHERE id = ?")
                .bind(at.clone())
                .bind(row.id)
                .execute(self.conn())
                .await?;
            delta.removed.push(row.name);
        }
        Ok(delta)
    }

    /// Returns `false` if there is no such tool.
    pub async fn set_tool_enabled(&mut self, id: i64, enabled: bool) -> Result<bool> {
        let r = self
            .q("UPDATE mcp_tools SET enabled = ? WHERE id = ?")
            .bind(flag(enabled))
            .bind(id)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Replaces every grant of the tool. The ids must be distinct and exist;
    /// the caller checks.
    pub async fn replace_tool_grants(&mut self, tool_id: i64, grants: &Grants) -> Result<()> {
        self.q("DELETE FROM mcp_tool_grants WHERE tool_id = ?")
            .bind(tool_id)
            .execute(self.conn())
            .await?;
        let rows = grants
            .everyone
            .then_some((None, None))
            .into_iter()
            .chain(grants.team_ids.iter().map(|t| (Some(*t), None)))
            .chain(grants.user_ids.iter().map(|u| (None, Some(*u))));
        for (team_id, user_id) in rows {
            self.q("INSERT INTO mcp_tool_grants (tool_id, team_id, user_id) VALUES (?, ?, ?)")
                .bind(tool_id)
                .bind(team_id)
                .bind(user_id)
                .execute(self.conn())
                .await?;
        }
        Ok(())
    }
}

impl Store {
    /// The grants of every tool of the server, by tool id.
    pub async fn mcp_grants_of_server(&self, server_id: i64) -> Result<HashMap<i64, Grants>> {
        let rows = self
            .q(
                "SELECT g.tool_id, g.team_id, g.user_id FROM mcp_tool_grants g
                 JOIN mcp_tools t ON t.id = g.tool_id
                 WHERE t.server_id = ? ORDER BY g.tool_id, g.team_id, g.user_id",
            )
            .bind(server_id)
            .fetch_all(self.pool())
            .await?;
        let mut by_tool: HashMap<i64, Vec<GrantRow>> = HashMap::new();
        for r in &rows {
            let g = grant_from(r);
            by_tool.entry(g.tool_id).or_default().push(GrantRow {
                model_id: g.tool_id,
                team_id: g.team_id,
                user_id: g.user_id,
            });
        }
        Ok(by_tool
            .into_iter()
            .map(|(id, rows)| (id, grants_of_rows(rows.into_iter())))
            .collect())
    }

    /// Every server, by name.
    pub async fn list_mcp_servers(&self) -> Result<Vec<McpServerRow>> {
        let mut conn = self.pool().acquire().await?;
        list_mcp_servers_in(&mut conn).await
    }

    pub async fn server_by_id(&self, id: i64) -> Result<Option<McpServerRow>> {
        let sql = format!("{SERVER_SELECT} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(server_from))
    }

    pub async fn tool_by_id(&self, id: i64) -> Result<Option<McpToolRow>> {
        let sql = format!("{TOOL_SELECT} WHERE id = ?");
        let row = self.q_dyn(sql).bind(id).fetch_optional(self.pool()).await?;
        Ok(row.as_ref().map(tool_from))
    }

    /// Every projection, by tool.
    pub async fn list_projections(&self) -> Result<Vec<ProjectionRow>> {
        let mut conn = self.pool().acquire().await?;
        list_projections_in(&mut conn).await
    }

    /// The projection of a tool, if it has one.
    pub async fn projection_of(&self, tool_id: i64) -> Result<Option<ProjectionRow>> {
        let sql = format!("{PROJECTION_SELECT} WHERE tool_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(tool_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(projection_from))
    }

    pub async fn server_by_name(&self, name: &str) -> Result<Option<McpServerRow>> {
        let sql = format!("{SERVER_SELECT} WHERE name = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(name)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(server_from))
    }

    /// The server's tools by name, removed ones included.
    pub async fn list_tools(&self, server_id: i64) -> Result<Vec<McpToolRow>> {
        let sql = format!("{TOOL_SELECT} WHERE server_id = ? ORDER BY name");
        let rows = self
            .q_dyn(sql)
            .bind(server_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(tool_from).collect())
    }

    pub async fn tool_grants_of(&self, tool_id: i64) -> Result<Grants> {
        let rows = self
            .q(
                "SELECT tool_id, team_id, user_id FROM mcp_tool_grants WHERE tool_id = ?
                 ORDER BY team_id, user_id",
            )
            .bind(tool_id)
            .fetch_all(self.pool())
            .await?;
        Ok(grants_of_rows(rows.iter().map(|r| {
            let g = grant_from(r);
            GrantRow {
                model_id: g.tool_id,
                team_id: g.team_id,
                user_id: g.user_id,
            }
        })))
    }
}

pub(crate) async fn list_mcp_servers_in(conn: &mut AnyConnection) -> Result<Vec<McpServerRow>> {
    let sql = format!("{SERVER_SELECT} WHERE org_id = ? ORDER BY name");
    let rows = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(server_from).collect())
}

/// Every tool still offered, switched on or not (the configuration export).
pub(crate) async fn list_offered_mcp_tools_in(conn: &mut AnyConnection) -> Result<Vec<McpToolRow>> {
    let sql = format!("{TOOL_SELECT} WHERE removed_at IS NULL ORDER BY server_id, name");
    let rows = conn.q_dyn(sql).fetch_all(&mut *conn).await?;
    Ok(rows.iter().map(tool_from).collect())
}

/// The grants of the tools [`list_offered_mcp_tools_in`] reads.
pub(crate) async fn list_offered_mcp_grants_in(
    conn: &mut AnyConnection,
) -> Result<Vec<McpGrantRow>> {
    let rows = conn
        .q(
            "SELECT g.tool_id, g.team_id, g.user_id FROM mcp_tool_grants g
             JOIN mcp_tools t ON t.id = g.tool_id
             WHERE t.removed_at IS NULL
             ORDER BY g.tool_id, g.team_id, g.user_id",
        )
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(grant_from).collect())
}

/// The projections of the tools [`list_offered_mcp_tools_in`] reads.
pub(crate) async fn list_projections_in(conn: &mut AnyConnection) -> Result<Vec<ProjectionRow>> {
    let sql = format!("{PROJECTION_SELECT} ORDER BY tool_id");
    let rows = conn.q_dyn(sql).fetch_all(&mut *conn).await?;
    Ok(rows.iter().map(projection_from).collect())
}

/// The tools a call could reach: switched on and still offered.
pub(crate) async fn list_live_mcp_tools_in(conn: &mut AnyConnection) -> Result<Vec<McpToolRow>> {
    let sql = format!("{TOOL_SELECT} WHERE enabled <> 0 AND removed_at IS NULL ORDER BY id");
    let rows = conn.q_dyn(sql).fetch_all(&mut *conn).await?;
    Ok(rows.iter().map(tool_from).collect())
}

/// The grants of the tools [`list_live_mcp_tools_in`] reads.
pub(crate) async fn list_live_mcp_grants_in(conn: &mut AnyConnection) -> Result<Vec<McpGrantRow>> {
    let rows = conn
        .q(
            "SELECT g.tool_id, g.team_id, g.user_id FROM mcp_tool_grants g
             JOIN mcp_tools t ON t.id = g.tool_id
             WHERE t.enabled <> 0 AND t.removed_at IS NULL
             ORDER BY g.tool_id, g.team_id, g.user_id",
        )
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(grant_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Grants, StoreError};

    fn server<'a>(name: &'a str) -> NewMcpServer<'a> {
        NewMcpServer {
            name,
            url: "https://mcp.example.com/mcp",
            auth_kind: "bearer",
            auth_header: None,
            auth_secret_enc: Some("enc:abc"),
            enabled: true,
            description: "d",
            ..Default::default()
        }
    }

    fn tool(name: &str, desc: &str) -> DiscoveredTool {
        DiscoveredTool {
            name: name.into(),
            title: None,
            description: desc.into(),
            input_schema: r#"{"type":"object"}"#.into(),
            output_schema: None,
            annotations: None,
        }
    }

    async fn sync(s: &Store, server: i64, tools: Vec<DiscoveredTool>) -> SyncDelta {
        let mut tx = s.begin().await.unwrap();
        let d = tx.upsert_tools(server, tools).await.unwrap();
        tx.commit().await.unwrap();
        d
    }

    async fn with_server(s: &Store) -> i64 {
        let mut tx = s.begin().await.unwrap();
        let id = tx.insert_server(server("files")).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    #[tokio::test]
    async fn servers_round_trip_and_names_are_unique() {
        let s = Store::open_in_memory().await.unwrap();
        let id = with_server(&s).await;
        let row = s.server_by_name("files").await.unwrap().unwrap();
        assert_eq!(row.id, id);
        assert_eq!(row.auth_secret_enc.as_deref(), Some("enc:abc"));
        assert!(row.enabled && row.last_sync_at.is_none());
        assert!(!format!("{row:?}").contains("enc:abc"));

        let mut tx = s.begin().await.unwrap();
        let dup = tx.insert_server(server("files")).await.unwrap_err();
        assert!(matches!(
            dup.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);

        let mut tx = s.begin().await.unwrap();
        let mut n = server("files");
        n.description = "new";
        n.enabled = false;
        assert!(tx.update_server(id, n).await.unwrap());
        assert!(tx.record_sync(id, Some("could not connect")).await.unwrap());
        tx.commit().await.unwrap();
        let row = s.server_by_id(id).await.unwrap().unwrap();
        assert_eq!(row.description, "new");
        assert!(!row.enabled);
        assert_eq!(row.last_sync_error.as_deref(), Some("could not connect"));
        assert!(row.last_sync_at.is_some());
        assert_eq!(s.list_mcp_servers().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_reports_the_delta() {
        let s = Store::open_in_memory().await.unwrap();
        let id = with_server(&s).await;
        let d = sync(&s, id, vec![tool("a", "one"), tool("b", "two")]).await;
        assert_eq!(d.added, ["a", "b"]);
        assert!(d.updated.is_empty() && d.removed.is_empty());
        let tools = s.list_tools(id).await.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().all(|t| !t.enabled && t.removed_at.is_none()));

        let again = sync(&s, id, vec![tool("a", "one"), tool("b", "two")]).await;
        assert_eq!(again, SyncDelta::default());
        assert_eq!(s.list_tools(id).await.unwrap().len(), 2);

        let mut tx = s.begin().await.unwrap();
        assert!(tx.set_tool_enabled(tools[0].id, true).await.unwrap());
        tx.commit().await.unwrap();
        let d = sync(&s, id, vec![tool("a", "changed"), tool("b", "two")]).await;
        assert_eq!(d.updated, ["a"]);
        let a = &s.list_tools(id).await.unwrap()[0];
        assert_eq!(a.description, "changed");
        assert!(a.enabled, "a sync keeps the switch");
    }

    #[tokio::test]
    async fn a_missing_tool_is_marked_removed_and_comes_back_cleared() {
        let s = Store::open_in_memory().await.unwrap();
        let id = with_server(&s).await;
        sync(&s, id, vec![tool("a", "x"), tool("b", "y")]).await;
        let d = sync(&s, id, vec![tool("a", "x")]).await;
        assert_eq!(d.removed, ["b"]);
        let b = s.list_tools(id).await.unwrap().remove(1);
        assert!(b.removed_at.is_some());
        // a second sync without it reports nothing new
        assert!(sync(&s, id, vec![tool("a", "x")]).await.removed.is_empty());

        let d = sync(&s, id, vec![tool("a", "x"), tool("b", "y")]).await;
        assert_eq!(d.updated, ["b"]);
        assert!(d.added.is_empty());
        let b = s.list_tools(id).await.unwrap().remove(1);
        assert!(b.removed_at.is_none());
    }

    #[tokio::test]
    async fn grants_survive_removal_and_the_snapshot_reads_enabled_live_tools() {
        let s = Store::open_in_memory().await.unwrap();
        let id = with_server(&s).await;
        sync(&s, id, vec![tool("a", "x"), tool("b", "y")]).await;
        let tools = s.list_tools(id).await.unwrap();
        let (a, b) = (tools[0].id, tools[1].id);
        let mut tx = s.begin().await.unwrap();
        let team = tx.insert_team("t").await.unwrap();
        for t in [a, b] {
            tx.set_tool_enabled(t, true).await.unwrap();
        }
        tx.replace_tool_grants(
            b,
            &Grants {
                everyone: true,
                team_ids: vec![team],
                user_ids: vec![],
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        sync(&s, id, vec![tool("a", "x")]).await;
        let g = s.tool_grants_of(b).await.unwrap();
        assert!(g.everyone);
        assert_eq!(g.team_ids, [team]);

        let rows = s.snapshot_rows().await.unwrap();
        assert_eq!(rows.mcp_servers.len(), 1);
        assert_eq!(rows.mcp_tools.iter().map(|t| t.id).collect::<Vec<_>>(), [a]);
        assert!(rows.mcp_tool_grants.is_empty(), "b is removed");

        sync(&s, id, vec![tool("a", "x"), tool("b", "y")]).await;
        let rows = s.snapshot_rows().await.unwrap();
        assert_eq!(rows.mcp_tools.len(), 2);
        assert_eq!(rows.mcp_tool_grants.len(), 2);
    }

    #[tokio::test]
    async fn deleting_a_server_takes_its_tools_and_grants() {
        let s = Store::open_in_memory().await.unwrap();
        let id = with_server(&s).await;
        sync(&s, id, vec![tool("a", "x")]).await;
        let a = s.list_tools(id).await.unwrap()[0].id;
        let mut tx = s.begin().await.unwrap();
        let team = tx.insert_team("t").await.unwrap();
        tx.replace_tool_grants(
            a,
            &Grants {
                everyone: false,
                team_ids: vec![team],
                user_ids: vec![],
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_server(id).await.unwrap());
        assert!(!tx.delete_server(id).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.list_tools(id).await.unwrap().is_empty());
        let n: i64 = s
            .scalar("SELECT COUNT(*) FROM mcp_tool_grants")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn a_tool_name_is_unique_per_server() {
        let s = Store::open_in_memory().await.unwrap();
        let id = with_server(&s).await;
        sync(&s, id, vec![tool("a", "x")]).await;
        let r = s
            .q("INSERT INTO mcp_tools (server_id, name, input_schema) VALUES (?, 'a', '{}')")
            .bind(id)
            .execute(s.pool())
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn a_local_server_round_trips_and_an_update_keeps_its_catalog_entry() {
        let store = Store::open_in_memory().await.unwrap();
        let args = vec!["-y".to_string(), "pkg".to_string()];
        let env = std::collections::BTreeMap::from([("A".to_string(), "enc-a".to_string())]);
        let mut tx = store.begin().await.unwrap();
        let id = tx
            .insert_server(NewMcpServer {
                name: "local-one",
                transport: "stdio",
                command: Some("npx"),
                args: &args,
                env_enc: &env,
                catalog_id: Some("fs"),
                ..Default::default()
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let row = store.server_by_id(id).await.unwrap().unwrap();
        assert!(row.is_local());
        assert_eq!(row.url, "");
        assert_eq!(row.command.as_deref(), Some("npx"));
        assert_eq!(row.args, args);
        assert_eq!(row.env_enc, env);
        assert_eq!(row.catalog_id.as_deref(), Some("fs"));
        let mut tx = store.begin().await.unwrap();
        let changed = vec!["other".to_string()];
        assert!(tx
            .update_server(
                id,
                NewMcpServer {
                    name: "local-one",
                    transport: "stdio",
                    command: Some("uvx"),
                    args: &changed,
                    env_enc: &env,
                    catalog_id: None,
                    ..Default::default()
                },
            )
            .await
            .unwrap());
        tx.commit().await.unwrap();
        let row = store.server_by_id(id).await.unwrap().unwrap();
        assert_eq!(row.command.as_deref(), Some("uvx"));
        assert_eq!(row.args, changed);
        assert_eq!(row.catalog_id.as_deref(), Some("fs"));
        // A server made before the columns existed reads as an HTTP one.
        let mut tx = store.begin().await.unwrap();
        let web = tx.insert_server(server("web")).await.unwrap();
        tx.commit().await.unwrap();
        let web = store.server_by_id(web).await.unwrap().unwrap();
        assert!(!web.is_local());
        assert!(web.args.is_empty() && web.env_enc.is_empty());
    }
}
