//! Identity types: roles, the request principal and email normalization.

pub mod limiter;
pub mod password;
pub mod policy;

use serde::{Deserialize, Serialize};

/// Longest accepted email address, in bytes.
const MAX_EMAIL_BYTES: usize = 254;

/// A user's role in the organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Member,
}

/// A user's role inside one team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole {
    Lead,
    Member,
}

/// The state of a user account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Invited,
    Disabled,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Role::Admin),
            "member" => Some(Role::Member),
            _ => None,
        }
    }
}

impl TeamRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamRole::Lead => "lead",
            TeamRole::Member => "member",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lead" => Some(TeamRole::Lead),
            "member" => Some(TeamRole::Member),
            _ => None,
        }
    }
}

impl UserStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            UserStatus::Active => "active",
            UserStatus::Invited => "invited",
            UserStatus::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(UserStatus::Active),
            "invited" => Some(UserStatus::Invited),
            "disabled" => Some(UserStatus::Disabled),
            _ => None,
        }
    }
}

/// Who is making an /api request, as of this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user_id: i64,
    pub email: String,
    pub role: Role,
    /// Teams the user belongs to, with their role in each.
    pub teams: Vec<(i64, TeamRole)>,
}

impl Principal {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// The ids of the teams the user belongs to.
    pub fn team_ids(&self) -> Vec<i64> {
        self.teams.iter().map(|(id, _)| *id).collect()
    }

    pub fn team_role(&self, team_id: i64) -> Option<TeamRole> {
        self.teams
            .iter()
            .find(|(id, _)| *id == team_id)
            .map(|(_, role)| *role)
    }

    pub fn leads(&self, team_id: i64) -> bool {
        self.team_role(team_id) == Some(TeamRole::Lead)
    }

    pub fn led_teams(&self) -> Vec<i64> {
        self.teams
            .iter()
            .filter(|(_, role)| *role == TeamRole::Lead)
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Trims and lowercases an email address, rejecting anything that is not
/// shaped like one.
pub fn normalize_email(raw: &str) -> Result<String, &'static str> {
    const INVALID: &str = "email is not valid";

    let email = raw.trim().to_lowercase();
    if email.len() > MAX_EMAIL_BYTES {
        return Err(INVALID);
    }
    if email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(INVALID);
    }
    let mut parts = email.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(INVALID);
    };
    if local.is_empty() || domain.is_empty() {
        return Err(INVALID);
    }
    if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
        return Err(INVALID);
    }
    Ok(email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip() {
        for role in [Role::Admin, Role::Member] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        for role in [TeamRole::Lead, TeamRole::Member] {
            assert_eq!(TeamRole::parse(role.as_str()), Some(role));
        }
        for status in [
            UserStatus::Active,
            UserStatus::Invited,
            UserStatus::Disabled,
        ] {
            assert_eq!(UserStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(Role::parse("ADMIN"), None);
        assert_eq!(Role::parse(""), None);
        assert_eq!(TeamRole::parse("LEAD"), None);
        assert_eq!(TeamRole::parse(""), None);
        assert_eq!(UserStatus::parse("ACTIVE"), None);
        assert_eq!(UserStatus::parse(""), None);
    }

    #[test]
    fn enums_serialize_lowercase() {
        assert_eq!(serde_json::to_string(&Role::Admin).unwrap(), "\"admin\"");
        assert_eq!(serde_json::to_string(&TeamRole::Lead).unwrap(), "\"lead\"");
        assert_eq!(
            serde_json::to_string(&UserStatus::Disabled).unwrap(),
            "\"disabled\""
        );
        assert_eq!(
            serde_json::from_str::<UserStatus>("\"invited\"").unwrap(),
            UserStatus::Invited
        );
    }

    #[test]
    fn principal_helpers() {
        let mut principal = Principal {
            user_id: 7,
            email: "maya@example.com".to_string(),
            role: Role::Member,
            teams: vec![(1, TeamRole::Lead), (2, TeamRole::Member)],
        };
        assert!(principal.leads(1));
        assert!(!principal.leads(2));
        assert!(!principal.leads(3));
        assert_eq!(principal.team_role(1), Some(TeamRole::Lead));
        assert_eq!(principal.team_role(2), Some(TeamRole::Member));
        assert_eq!(principal.team_role(3), None);
        assert_eq!(principal.led_teams(), vec![1]);
        assert!(!principal.is_admin());
        principal.role = Role::Admin;
        assert!(principal.is_admin());
    }

    #[test]
    fn email_is_normalized() {
        assert_eq!(
            normalize_email("  Maya@Example.COM "),
            Ok("maya@example.com".to_string())
        );
    }

    #[test]
    fn invalid_emails_are_rejected() {
        let too_long = format!("{}@example.com", "a".repeat(255 - "@example.com".len()));
        assert_eq!(too_long.len(), 255);
        let cases = [
            "",
            "a",
            "a@",
            "@b.co",
            "a@b",
            "a@.co",
            "a@b.",
            "a b@c.co",
            "a@b@c.co",
            "a\n@b.co",
            too_long.as_str(),
        ];
        for case in cases {
            assert_eq!(
                normalize_email(case),
                Err("email is not valid"),
                "{case:?} should be rejected"
            );
        }
    }

    #[test]
    fn longest_email_is_accepted() {
        let longest = format!("{}@example.com", "a".repeat(254 - "@example.com".len()));
        assert_eq!(longest.len(), 254);
        assert_eq!(normalize_email(&longest), Ok(longest.clone()));
    }
}
