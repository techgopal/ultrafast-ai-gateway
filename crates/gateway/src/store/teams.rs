//! Teams and their members.

use anyhow::{anyhow, Result};
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};

use super::{write_error, Store, Tx, DEFAULT_ORG};
use crate::identity::TeamRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub team_id: i64,
    pub user_id: i64,
    pub role: TeamRole,
}

/// A team with the number of its members.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct TeamSummary {
    pub id: i64,
    pub name: String,
    pub member_count: i64,
    pub created_at: String,
}

/// A member of a team, with what names them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct MemberDetail {
    pub user_id: i64,
    pub email: String,
    pub name: String,
    pub role: TeamRole,
}

const SUMMARY_SELECT: &str = "SELECT t.id, t.name, t.created_at,
            (SELECT COUNT(*) FROM team_members m
             WHERE m.team_id = t.id AND m.org_id = t.org_id) AS member_count
     FROM teams t";

fn summary_from(r: &SqliteRow) -> TeamSummary {
    TeamSummary {
        id: r.get("id"),
        name: r.get("name"),
        member_count: r.get("member_count"),
        created_at: r.get("created_at"),
    }
}

fn team_from(r: &SqliteRow) -> TeamRow {
    TeamRow {
        id: r.get("id"),
        name: r.get("name"),
        created_at: r.get("created_at"),
    }
}

fn member_from(r: &SqliteRow) -> Result<MemberRow> {
    let role: String = r.get("role");
    Ok(MemberRow {
        team_id: r.get("team_id"),
        user_id: r.get("user_id"),
        role: TeamRole::parse(&role).ok_or_else(|| anyhow!("stored team role is not known"))?,
    })
}

impl Store {
    pub async fn team_by_id(&self, id: i64) -> Result<Option<TeamRow>> {
        let row = sqlx::query("SELECT id, name, created_at FROM teams WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(team_from))
    }

    /// Ordered by name.
    pub async fn list_teams(&self) -> Result<Vec<TeamRow>> {
        let rows =
            sqlx::query("SELECT id, name, created_at FROM teams WHERE org_id = ? ORDER BY name")
                .bind(DEFAULT_ORG)
                .fetch_all(self.pool())
                .await?;
        Ok(rows.iter().map(team_from).collect())
    }

    pub async fn team_summary(&self, id: i64) -> Result<Option<TeamSummary>> {
        let sql = format!("{SUMMARY_SELECT} WHERE t.id = ? AND t.org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(summary_from))
    }

    /// Every team, ordered by name.
    pub async fn list_team_summaries(&self) -> Result<Vec<TeamSummary>> {
        let sql = format!("{SUMMARY_SELECT} WHERE t.org_id = ? ORDER BY t.name");
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(summary_from).collect())
    }

    /// The teams the user belongs to in any role, ordered by name.
    pub async fn list_team_summaries_of(&self, user_id: i64) -> Result<Vec<TeamSummary>> {
        let sql = format!(
            "{SUMMARY_SELECT}
             JOIN team_members own ON own.team_id = t.id AND own.org_id = t.org_id
             WHERE t.org_id = ? AND own.user_id = ?
             ORDER BY t.name"
        );
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .bind(user_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(summary_from).collect())
    }

    /// The members of a team with their email and name, ordered by email.
    pub async fn member_details(&self, team_id: i64) -> Result<Vec<MemberDetail>> {
        let rows = sqlx::query(
            "SELECT m.user_id, u.email, u.name, m.role
             FROM team_members m
             JOIN users u ON u.id = m.user_id AND u.org_id = m.org_id
             WHERE m.team_id = ? AND m.org_id = ?
             ORDER BY u.email",
        )
        .bind(team_id)
        .bind(DEFAULT_ORG)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|r| {
                let role: String = r.get("role");
                Ok(MemberDetail {
                    user_id: r.get("user_id"),
                    email: r.get("email"),
                    name: r.get("name"),
                    role: TeamRole::parse(&role)
                        .ok_or_else(|| anyhow!("stored team role is not known"))?,
                })
            })
            .collect()
    }

    /// Ordered by user id.
    pub async fn members_of(&self, team_id: i64) -> Result<Vec<MemberRow>> {
        let rows = sqlx::query(
            "SELECT team_id, user_id, role FROM team_members
             WHERE team_id = ? AND org_id = ? ORDER BY user_id",
        )
        .bind(team_id)
        .bind(DEFAULT_ORG)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(member_from).collect()
    }

    /// Ordered by team id.
    pub async fn memberships_of(&self, user_id: i64) -> Result<Vec<MemberRow>> {
        let rows = sqlx::query(
            "SELECT team_id, user_id, role FROM team_members
             WHERE user_id = ? AND org_id = ? ORDER BY team_id",
        )
        .bind(user_id)
        .bind(DEFAULT_ORG)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(member_from).collect()
    }
}

impl Tx<'_> {
    /// The team as the transaction sees it.
    pub async fn team_by_id(&mut self, id: i64) -> Result<Option<TeamRow>> {
        let row = sqlx::query("SELECT id, name, created_at FROM teams WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(team_from))
    }

    /// The user's role in the team, as the transaction sees it.
    pub async fn member_role(&mut self, team_id: i64, user_id: i64) -> Result<Option<TeamRole>> {
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM team_members WHERE team_id = ? AND user_id = ? AND org_id = ?",
        )
        .bind(team_id)
        .bind(user_id)
        .bind(DEFAULT_ORG)
        .fetch_optional(self.conn())
        .await?;
        role.map(|r| TeamRole::parse(&r).ok_or_else(|| anyhow!("stored team role is not known")))
            .transpose()
    }

    /// Fails with `StoreError::Duplicate` when the name is taken.
    pub async fn insert_team(&mut self, name: &str) -> Result<i64> {
        let r = sqlx::query("INSERT INTO teams (org_id, name) VALUES (?, ?)")
            .bind(DEFAULT_ORG)
            .bind(name)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.last_insert_rowid())
    }

    /// Fails with `StoreError::Duplicate` when the name is taken.
    pub async fn rename_team(&mut self, id: i64, name: &str) -> Result<bool> {
        let r = sqlx::query("UPDATE teams SET name = ? WHERE id = ? AND org_id = ?")
            .bind(name)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    /// Also removes the team's memberships and detaches its keys.
    pub async fn delete_team(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM teams WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Adds the user to the team, or changes their role if already a member.
    pub async fn put_member(&mut self, team_id: i64, user_id: i64, role: TeamRole) -> Result<()> {
        sqlx::query(
            "INSERT INTO team_members (org_id, team_id, user_id, role) VALUES (?, ?, ?, ?)
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = excluded.role
             WHERE team_members.org_id = excluded.org_id",
        )
        .bind(DEFAULT_ORG)
        .bind(team_id)
        .bind(user_id)
        .bind(role.as_str())
        .execute(self.conn())
        .await?;
        Ok(())
    }

    pub async fn remove_member(&mut self, team_id: i64, user_id: i64) -> Result<bool> {
        let r = sqlx::query(
            "DELETE FROM team_members WHERE team_id = ? AND user_id = ? AND org_id = ?",
        )
        .bind(team_id)
        .bind(user_id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Role, UserStatus};
    use crate::store::{check_timestamp, NewUser, StoreError};

    fn member(email: &str) -> NewUser<'_> {
        NewUser {
            email,
            name: "Someone",
            role: Role::Member,
            status: UserStatus::Active,
            password_hash: None,
        }
    }

    #[tokio::test]
    async fn summaries_details_and_reads_in_a_transaction() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let one = tx.insert_team("one").await.unwrap();
        let two = tx.insert_team("two").await.unwrap();
        let empty = tx.insert_team("empty").await.unwrap();
        let maya = tx.insert_user(member("maya@example.com")).await.unwrap();
        let noor = tx.insert_user(member("noor@example.com")).await.unwrap();
        tx.put_member(one, noor, TeamRole::Lead).await.unwrap();
        tx.put_member(one, maya, TeamRole::Member).await.unwrap();
        tx.put_member(two, noor, TeamRole::Member).await.unwrap();
        assert_eq!(tx.team_by_id(one).await.unwrap().unwrap().name, "one");
        assert!(tx.team_by_id(empty + 1).await.unwrap().is_none());
        assert_eq!(
            tx.member_role(one, noor).await.unwrap(),
            Some(TeamRole::Lead)
        );
        assert_eq!(tx.member_role(two, maya).await.unwrap(), None);
        tx.commit().await.unwrap();

        let counts = |rows: Vec<TeamSummary>| {
            rows.into_iter()
                .map(|t| (t.name, t.member_count))
                .collect::<Vec<_>>()
        };
        let pair = |name: &str, n: i64| (name.to_string(), n);
        assert_eq!(
            counts(s.list_team_summaries().await.unwrap()),
            [pair("empty", 0), pair("one", 2), pair("two", 1)]
        );
        assert_eq!(
            counts(s.list_team_summaries_of(noor).await.unwrap()),
            [pair("one", 2), pair("two", 1)]
        );
        assert_eq!(
            counts(s.list_team_summaries_of(maya).await.unwrap()),
            [pair("one", 2)]
        );
        let summary = s.team_summary(one).await.unwrap().unwrap();
        assert_eq!((summary.id, summary.member_count), (one, 2));
        assert!(check_timestamp(&summary.created_at).is_ok());
        assert!(s.team_summary(empty + 1).await.unwrap().is_none());

        let details = s.member_details(one).await.unwrap();
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].email, "maya@example.com");
        assert_eq!(details[0].role, TeamRole::Member);
        assert_eq!(details[1].user_id, noor);
        assert_eq!(details[1].role, TeamRole::Lead);
        assert!(s.member_details(empty).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn team_round_trip_and_duplicate() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let platform = tx.insert_team("platform").await.unwrap();
        let data = tx.insert_team("data").await.unwrap();
        let err = tx
            .insert_team("platform")
            .await
            .expect_err("a second team with the same name");
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        tx.commit().await.unwrap();

        let t = s.team_by_id(platform).await.unwrap().unwrap();
        assert_eq!(t.id, platform);
        assert_eq!(t.name, "platform");
        assert!(check_timestamp(&t.created_at).is_ok());
        assert!(s.team_by_id(platform + data + 1).await.unwrap().is_none());

        let names: Vec<String> = s
            .list_teams()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["data", "platform"]);
    }

    #[tokio::test]
    async fn team_rename_and_delete_report_changes() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let a = tx.insert_team("a").await.unwrap();
        tx.insert_team("b").await.unwrap();
        assert!(tx.rename_team(a, "c").await.unwrap());
        assert!(!tx.rename_team(a + 1000, "d").await.unwrap());
        let err = tx.rename_team(a, "b").await.expect_err("name is taken");
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        assert!(!tx.delete_team(a + 1000).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(s.team_by_id(a).await.unwrap().unwrap().name, "c");
    }

    #[tokio::test]
    async fn membership() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let team = tx.insert_team("platform").await.unwrap();
        let other = tx.insert_team("data").await.unwrap();
        let maya = tx.insert_user(member("maya@example.com")).await.unwrap();
        let noor = tx.insert_user(member("noor@example.com")).await.unwrap();
        tx.put_member(team, maya, TeamRole::Member).await.unwrap();
        tx.put_member(team, noor, TeamRole::Member).await.unwrap();
        tx.put_member(other, maya, TeamRole::Member).await.unwrap();
        tx.put_member(team, maya, TeamRole::Lead).await.unwrap();
        assert!(tx
            .put_member(team, noor + 1000, TeamRole::Lead)
            .await
            .is_err());
        tx.commit().await.unwrap();

        let lead = MemberRow {
            team_id: team,
            user_id: maya,
            role: TeamRole::Lead,
        };
        assert_eq!(
            s.members_of(team).await.unwrap(),
            [
                lead.clone(),
                MemberRow {
                    team_id: team,
                    user_id: noor,
                    role: TeamRole::Member,
                },
            ]
        );
        assert_eq!(
            s.memberships_of(maya).await.unwrap(),
            [
                lead,
                MemberRow {
                    team_id: other,
                    user_id: maya,
                    role: TeamRole::Member,
                },
            ]
        );

        let mut tx = s.begin().await.unwrap();
        assert!(tx.remove_member(team, maya).await.unwrap());
        assert!(!tx.remove_member(team, maya).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(s.members_of(team).await.unwrap().len(), 1);
        assert_eq!(s.memberships_of(maya).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deleting_a_team_keeps_keys() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let team = tx.insert_team("platform").await.unwrap();
        let maya = tx.insert_user(member("maya@example.com")).await.unwrap();
        tx.put_member(team, maya, TeamRole::Member).await.unwrap();
        tx.insert_key("k", "h", "uf-sk-…aaaa", None, None, Some(team))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            s.active_key_by_hash("h").await.unwrap().unwrap().team_id,
            Some(team)
        );

        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_team(team).await.unwrap());
        tx.commit().await.unwrap();

        assert!(s.team_by_id(team).await.unwrap().is_none());
        assert!(s.members_of(team).await.unwrap().is_empty());
        assert!(s.user_by_id(maya).await.unwrap().is_some());
        let key = s.active_key_by_hash("h").await.unwrap().unwrap();
        assert_eq!(key.team_id, None);
    }

    #[tokio::test]
    async fn teams_are_scoped_to_the_org() {
        let s = Store::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO teams (org_id, name) VALUES (2, 'other')")
            .execute(s.pool())
            .await
            .unwrap();
        assert!(s.team_by_id(1).await.unwrap().is_none());
        assert!(s.list_teams().await.unwrap().is_empty());
        let mut tx = s.begin().await.unwrap();
        assert!(!tx.rename_team(1, "x").await.unwrap());
        assert!(!tx.delete_team(1).await.unwrap());
    }
}
