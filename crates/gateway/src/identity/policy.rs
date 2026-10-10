//! Authorization policy: the one place that decides what a principal may do.

use super::{Principal, Role, TeamRole};
use crate::limits::LimitScope;

/// What a request wants to do, with the facts needed to decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // users
    ListUsers,
    ViewUser {
        user_id: i64,
        shares_led_team: bool,
    },
    InviteUser {
        role: Role,
    },
    UpdateUser {
        user_id: i64,
        changes_role_or_status: bool,
    },
    DeleteUser {
        user_id: i64,
    },
    // teams
    ListTeams,
    CreateTeam,
    ViewTeam {
        team_id: i64,
    },
    RenameTeam {
        team_id: i64,
    },
    DeleteTeam {
        team_id: i64,
    },
    /// Adds a user to a team as a member, by email.
    AddMember {
        team_id: i64,
    },
    /// Adds a user or sets their role. Only an admin may.
    PutMember {
        team_id: i64,
    },
    /// `target_role` is the role the target has in the team, `None` when
    /// they are not in it.
    RemoveMember {
        team_id: i64,
        target_user_id: i64,
        target_role: Option<TeamRole>,
    },
    // virtual keys
    ListKeys,
    CreateKey {
        owner_id: i64,
        team_id: Option<i64>,
    },
    ViewKey {
        owner_id: Option<i64>,
        team_id: Option<i64>,
    },
    RevokeKey {
        owner_id: Option<i64>,
        team_id: Option<i64>,
    },
    /// Replaces the tags of a key. Admins only: a key's tags win over a
    /// call's, so they are the admin's labels, and an owner may not take
    /// them off. The creator sets tags when the key is made.
    EditKeyTags {
        owner_id: Option<i64>,
        team_id: Option<i64>,
    },
    // access tokens: always the caller's own
    ManageOwnTokens,
    // providers
    ListProviders,
    ManageProviders,
    // models
    ListModels,
    ManageModels,
    // routes
    ListRoutes,
    ManageRoutes,
    // audit
    ViewAudit,
    // routing
    ViewRoutingHealth,
    // alerts: channels, rules and events. Admins only.
    ManageAlerts,
    // guardrails: defining them and attaching them to routes and keys.
    // Admins only; a lead sees the ones on their keys through the key.
    ManageGuardrails,
    // prompt templates
    /// Reading templates and rendering them: everyone signed in, since
    /// anyone who can call may use any template by name.
    ListPrompts,
    /// Making a template. Admins, and team leads.
    CreatePrompt,
    /// Adding a version to a template or deleting it. `created_by` is the
    /// user who made the template: admins manage all, a lead the ones they
    /// made, nobody else any.
    ManagePrompt {
        created_by: Option<i64>,
    },
    // settings: viewing and changing both
    ManageSettings,
    // playground
    /// Every signed-in user: what they may call is decided as for their keys.
    UsePlayground,
    // rate limits
    /// Everyone may ask; what they get is cut by `limit_access`.
    ListLimits,
    /// Setting and removing limits. Only an admin may.
    ManageLimits,
    // budgets
    /// Everyone may ask; what they get is cut by `limit_access`.
    ListBudgets,
    /// Setting and removing budgets. Only an admin may.
    ManageBudgets,
    // request logs
    /// Everyone may ask; what they get is cut to `list_scope`.
    ListLogs,
    /// Everyone may ask; the sums are cut to `list_scope`, as the log list is.
    ListUsage,
    /// `user_in_led_team`: the row's user is a member of a team the caller
    /// leads. It only counts for a caller who leads a team.
    ViewLog {
        user_id: Option<i64>,
        team_id: Option<i64>,
        user_in_led_team: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Answer 403.
    Forbidden,
    /// Answer 404, so the caller cannot learn that the target exists.
    Hidden,
}

/// Which rows a list call may return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Teams {
        team_ids: Vec<i64>,
        own_user_id: i64,
    },
    Own {
        user_id: i64,
    },
}

/// Decides whether `p` may perform `action`. Pure: no I/O, no clock.
///
/// The match has no catch-all arm on purpose: a new `Action` variant must be
/// given a rule before the crate compiles.
pub fn authorize(p: &Principal, action: &Action) -> Decision {
    use Decision::{Allow, Forbidden, Hidden};

    if p.is_admin() {
        return Allow;
    }
    match action {
        Action::ListUsers
        | Action::ListTeams
        | Action::ListKeys
        | Action::ManageOwnTokens
        | Action::ListProviders
        | Action::ListModels
        | Action::ListRoutes
        | Action::ListLogs
        | Action::ListLimits
        | Action::ListBudgets
        | Action::ListUsage
        | Action::ListPrompts
        | Action::UsePlayground => Allow,

        Action::InviteUser { role: _ }
        | Action::CreateTeam
        | Action::ManageProviders
        | Action::ManageModels
        | Action::ManageRoutes
        | Action::ViewAudit
        | Action::ViewRoutingHealth
        | Action::ManageAlerts
        | Action::ManageGuardrails
        | Action::ManageSettings
        | Action::ManageLimits
        | Action::ManageBudgets => Forbidden,

        Action::ViewUser {
            user_id,
            shares_led_team,
        } => {
            // `shares_led_team` only counts for a caller who leads a team.
            let leads_any = !p.led_teams().is_empty();
            if *user_id == p.user_id || (leads_any && *shares_led_team) {
                Allow
            } else {
                Hidden
            }
        }
        Action::UpdateUser {
            user_id,
            changes_role_or_status,
        } => {
            if *user_id != p.user_id {
                Hidden
            } else if *changes_role_or_status {
                Forbidden
            } else {
                Allow
            }
        }
        Action::DeleteUser { user_id } => {
            if *user_id == p.user_id {
                Forbidden
            } else {
                Hidden
            }
        }

        Action::CreatePrompt => {
            if p.led_teams().is_empty() {
                Forbidden
            } else {
                Allow
            }
        }
        Action::ManagePrompt { created_by } => {
            if created_by.is_some_and(|c| c == p.user_id) && !p.led_teams().is_empty() {
                Allow
            } else {
                Forbidden
            }
        }

        Action::ViewTeam { team_id } => by_team_role(p, *team_id, Allow, Allow),
        Action::RenameTeam { team_id } => by_team_role(p, *team_id, Allow, Forbidden),
        Action::DeleteTeam { team_id } => by_team_role(p, *team_id, Forbidden, Forbidden),
        Action::AddMember { team_id } => by_team_role(p, *team_id, Allow, Forbidden),
        // Role changes are the admin's: nobody else may make a lead.
        Action::PutMember { team_id } => by_team_role(p, *team_id, Forbidden, Forbidden),
        Action::RemoveMember {
            team_id,
            target_user_id,
            target_role,
        } => {
            // A lead removes members and may leave, never remove another lead.
            let lead = if *target_role == Some(TeamRole::Lead) && *target_user_id != p.user_id {
                Forbidden
            } else {
                Allow
            };
            by_team_role(p, *team_id, lead, Forbidden)
        }

        Action::ViewLog {
            user_id,
            team_id,
            user_in_led_team,
        } => {
            if *user_id == Some(p.user_id)
                || team_id.is_some_and(|t| p.leads(t))
                || (*user_in_led_team && !p.led_teams().is_empty())
            {
                Allow
            } else {
                Hidden
            }
        }

        Action::CreateKey { owner_id, team_id } => {
            let allowed = if *owner_id == p.user_id {
                team_id.is_none_or(|t| p.team_role(t).is_some())
            } else {
                team_id.is_some_and(|t| p.leads(t))
            };
            if allowed {
                Allow
            } else {
                Forbidden
            }
        }
        // Who may see the key is told it is not theirs to edit; the rest,
        // that it is not there.
        Action::EditKeyTags { owner_id, team_id } => {
            if *owner_id == Some(p.user_id) || team_id.is_some_and(|t| p.leads(t)) {
                Forbidden
            } else {
                Hidden
            }
        }
        Action::ViewKey { owner_id, team_id } | Action::RevokeKey { owner_id, team_id } => {
            if *owner_id == Some(p.user_id) || team_id.is_some_and(|t| p.leads(t)) {
                Allow
            } else {
                Hidden
            }
        }
    }
}

/// The decision for a team action: one outcome for the team's lead, one for
/// its other members, and `Hidden` for everyone outside the team.
fn by_team_role(p: &Principal, team_id: i64, lead: Decision, member: Decision) -> Decision {
    match p.team_role(team_id) {
        Some(TeamRole::Lead) => lead,
        Some(TeamRole::Member) => member,
        None => Decision::Hidden,
    }
}

/// What a caller may see of a limit or budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitAccess {
    /// Not listed.
    Hidden,
    /// Listed with what was set (amount, period, action) but not what was
    /// spent: the gateway's and the caller's teams', unless they lead the team.
    Figures,
    /// Listed with what was spent: the caller's own user and keys, the teams
    /// they lead, and everything for an admin.
    Spent,
}

/// What `p` sees of a limit or budget. An admin sees all with the spend.
/// Anyone else sees the gateway's and those of their teams without the
/// spend (with it for a team they lead), and their own user's and own keys'
/// with the spend; nothing of another person's user or keys, so no one's
/// spend is shown to a colleague. `key_owner` is the owner of the key, for
/// a key limit.
pub fn limit_access(
    p: &Principal,
    scope: LimitScope,
    scope_id: Option<i64>,
    key_owner: Option<i64>,
) -> LimitAccess {
    if p.is_admin() {
        return LimitAccess::Spent;
    }
    match scope {
        LimitScope::Gateway => LimitAccess::Figures,
        LimitScope::Team => match scope_id.and_then(|t| p.team_role(t)) {
            Some(TeamRole::Lead) => LimitAccess::Spent,
            Some(TeamRole::Member) => LimitAccess::Figures,
            None => LimitAccess::Hidden,
        },
        LimitScope::User if scope_id == Some(p.user_id) => LimitAccess::Spent,
        LimitScope::Key if key_owner == Some(p.user_id) => LimitAccess::Spent,
        LimitScope::User | LimitScope::Key => LimitAccess::Hidden,
    }
}

/// Whether a key that `p` makes for `owner_id` is a team key: one a
/// non-admin makes for another user. It acts for its team only: it calls
/// what is granted to everyone or to its team, never its owner's own grants
/// or an admin's reach (see `access`).
pub fn key_for_another_is_team_key(p: &Principal, owner_id: i64) -> bool {
    !p.is_admin() && owner_id != p.user_id
}

/// Whether a user may own a team key that a lead makes: only a plain member
/// of the team, never one of its leads or an admin. `owner_team_role` is
/// their role in the key's team.
pub fn may_own_team_key(owner_role: Role, owner_team_role: Option<TeamRole>) -> bool {
    owner_role != Role::Admin && owner_team_role == Some(TeamRole::Member)
}

/// Which rows a list call by `p` may return.
pub fn list_scope(p: &Principal) -> Scope {
    if p.is_admin() {
        return Scope::All;
    }
    let team_ids = p.led_teams();
    if team_ids.is_empty() {
        Scope::Own { user_id: p.user_id }
    } else {
        Scope::Teams {
            team_ids,
            own_user_id: p.user_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Decision::{Allow, Forbidden, Hidden};
    use LimitAccess::{Figures, Hidden as Hide, Spent};

    type Case<'a> = (&'static str, &'a Principal, Action, Decision);

    struct Fixture {
        admin: Principal,
        lead: Principal,
        member: Principal,
        loner: Principal,
    }

    fn principal(user_id: i64, role: Role, teams: Vec<(i64, TeamRole)>) -> Principal {
        Principal {
            user_id,
            email: format!("user{user_id}@example.com"),
            role,
            teams,
        }
    }

    fn fixture() -> Fixture {
        Fixture {
            admin: principal(1, Role::Admin, vec![]),
            lead: principal(
                2,
                Role::Member,
                vec![(10, TeamRole::Lead), (20, TeamRole::Member)],
            ),
            member: principal(3, Role::Member, vec![(10, TeamRole::Member)]),
            loner: principal(4, Role::Member, vec![]),
        }
    }

    /// The rows shared by `ViewKey` and `RevokeKey`, which have the same rule.
    fn key_cases<'a>(
        f: &'a Fixture,
        view: bool,
        make: fn(Option<i64>, Option<i64>) -> Action,
    ) -> Vec<Case<'a>> {
        let rows: Vec<(&'static str, &'static str, &Principal, Action, Decision)> = vec![
            (
                "view_key: lead, own key without a team",
                "revoke_key: lead, own key without a team",
                &f.lead,
                make(Some(2), None),
                Allow,
            ),
            (
                "view_key: lead, own key in a team they belong to",
                "revoke_key: lead, own key in a team they belong to",
                &f.lead,
                make(Some(2), Some(20)),
                Allow,
            ),
            (
                "view_key: lead, own key in an unrelated team",
                "revoke_key: lead, own key in an unrelated team",
                &f.lead,
                make(Some(2), Some(30)),
                Allow,
            ),
            (
                "view_key: lead, another user's key in a led team",
                "revoke_key: lead, another user's key in a led team",
                &f.lead,
                make(Some(3), Some(10)),
                Allow,
            ),
            (
                "view_key: lead, ownerless key in a led team",
                "revoke_key: lead, ownerless key in a led team",
                &f.lead,
                make(None, Some(10)),
                Allow,
            ),
            (
                "view_key: lead, another user's key in a team they only belong to",
                "revoke_key: lead, another user's key in a team they only belong to",
                &f.lead,
                make(Some(3), Some(20)),
                Hidden,
            ),
            (
                "view_key: lead, ownerless key in a team they only belong to",
                "revoke_key: lead, ownerless key in a team they only belong to",
                &f.lead,
                make(None, Some(20)),
                Hidden,
            ),
            (
                "view_key: lead, another user's key in an unrelated team",
                "revoke_key: lead, another user's key in an unrelated team",
                &f.lead,
                make(Some(3), Some(30)),
                Hidden,
            ),
            (
                "view_key: lead, another user's key without a team",
                "revoke_key: lead, another user's key without a team",
                &f.lead,
                make(Some(3), None),
                Hidden,
            ),
            (
                "view_key: lead, key with no owner and no team",
                "revoke_key: lead, key with no owner and no team",
                &f.lead,
                make(None, None),
                Hidden,
            ),
            (
                "view_key: member, own key without a team",
                "revoke_key: member, own key without a team",
                &f.member,
                make(Some(3), None),
                Allow,
            ),
            (
                "view_key: member, own key in their team",
                "revoke_key: member, own key in their team",
                &f.member,
                make(Some(3), Some(10)),
                Allow,
            ),
            (
                "view_key: member, another user's key in their team",
                "revoke_key: member, another user's key in their team",
                &f.member,
                make(Some(2), Some(10)),
                Hidden,
            ),
            (
                "view_key: member, ownerless key in their team",
                "revoke_key: member, ownerless key in their team",
                &f.member,
                make(None, Some(10)),
                Hidden,
            ),
            (
                "view_key: member, another user's key in an unrelated team",
                "revoke_key: member, another user's key in an unrelated team",
                &f.member,
                make(Some(2), Some(30)),
                Hidden,
            ),
            (
                "view_key: loner, own key",
                "revoke_key: loner, own key",
                &f.loner,
                make(Some(4), None),
                Allow,
            ),
            (
                "view_key: loner, another user's key",
                "revoke_key: loner, another user's key",
                &f.loner,
                make(Some(2), None),
                Hidden,
            ),
            (
                "view_key: loner, another user's key in a team",
                "revoke_key: loner, another user's key in a team",
                &f.loner,
                make(Some(3), Some(10)),
                Hidden,
            ),
            (
                "view_key: loner, key with no owner and no team",
                "revoke_key: loner, key with no owner and no team",
                &f.loner,
                make(None, None),
                Hidden,
            ),
        ];
        rows.into_iter()
            .map(|(view_name, revoke_name, p, action, want)| {
                (if view { view_name } else { revoke_name }, p, action, want)
            })
            .collect()
    }

    fn cases(f: &Fixture) -> Vec<Case<'_>> {
        let (lead, member, loner) = (&f.lead, &f.member, &f.loner);
        let view_user = |user_id, shares_led_team| Action::ViewUser {
            user_id,
            shares_led_team,
        };
        let update_user = |user_id, changes_role_or_status| Action::UpdateUser {
            user_id,
            changes_role_or_status,
        };
        let put = |team_id| Action::PutMember { team_id };
        let add = |team_id| Action::AddMember { team_id };
        let remove = |team_id, target_user_id, target_role| Action::RemoveMember {
            team_id,
            target_user_id,
            target_role,
        };
        let create_key = |owner_id, team_id| Action::CreateKey { owner_id, team_id };

        let mut cases: Vec<Case<'_>> = vec![
            // ListUsers
            ("list_users: lead", lead, Action::ListUsers, Allow),
            ("list_users: member", member, Action::ListUsers, Allow),
            ("list_users: loner", loner, Action::ListUsers, Allow),
            // ViewUser
            ("view_user: lead, own id", lead, view_user(2, false), Allow),
            (
                "view_user: lead, user sharing a led team",
                lead,
                view_user(3, true),
                Allow,
            ),
            (
                "view_user: lead, user not sharing a led team",
                lead,
                view_user(4, false),
                Hidden,
            ),
            (
                "view_user: member, own id",
                member,
                view_user(3, false),
                Allow,
            ),
            (
                "view_user: member, another user",
                member,
                view_user(2, false),
                Hidden,
            ),
            (
                "view_user: INTERPRETED member who leads nothing, shares_led_team set",
                member,
                view_user(2, true),
                Hidden,
            ),
            (
                "view_user: loner, own id",
                loner,
                view_user(4, false),
                Allow,
            ),
            (
                "view_user: loner, another user",
                loner,
                view_user(2, false),
                Hidden,
            ),
            (
                "view_user: INTERPRETED loner, shares_led_team set",
                loner,
                view_user(2, true),
                Hidden,
            ),
            // InviteUser
            (
                "invite_user: lead inviting a member",
                lead,
                Action::InviteUser { role: Role::Member },
                Forbidden,
            ),
            (
                "invite_user: lead inviting an admin",
                lead,
                Action::InviteUser { role: Role::Admin },
                Forbidden,
            ),
            (
                "invite_user: member",
                member,
                Action::InviteUser { role: Role::Member },
                Forbidden,
            ),
            (
                "invite_user: loner",
                loner,
                Action::InviteUser { role: Role::Member },
                Forbidden,
            ),
            // UpdateUser
            (
                "update_user: lead, own profile",
                lead,
                update_user(2, false),
                Allow,
            ),
            (
                "update_user: lead, own role or status",
                lead,
                update_user(2, true),
                Forbidden,
            ),
            (
                "update_user: lead, another user's profile",
                lead,
                update_user(3, false),
                Hidden,
            ),
            (
                "update_user: lead, another user's role or status",
                lead,
                update_user(3, true),
                Hidden,
            ),
            (
                "update_user: member, own profile",
                member,
                update_user(3, false),
                Allow,
            ),
            (
                "update_user: member, own role or status",
                member,
                update_user(3, true),
                Forbidden,
            ),
            (
                "update_user: member, another user",
                member,
                update_user(2, false),
                Hidden,
            ),
            (
                "update_user: loner, own profile",
                loner,
                update_user(4, false),
                Allow,
            ),
            (
                "update_user: loner, own role or status",
                loner,
                update_user(4, true),
                Forbidden,
            ),
            (
                "update_user: loner, another user's role or status",
                loner,
                update_user(2, true),
                Hidden,
            ),
            // DeleteUser
            (
                "delete_user: lead, own id",
                lead,
                Action::DeleteUser { user_id: 2 },
                Forbidden,
            ),
            (
                "delete_user: lead, a user in a led team",
                lead,
                Action::DeleteUser { user_id: 3 },
                Hidden,
            ),
            (
                "delete_user: member, own id",
                member,
                Action::DeleteUser { user_id: 3 },
                Forbidden,
            ),
            (
                "delete_user: member, another user",
                member,
                Action::DeleteUser { user_id: 2 },
                Hidden,
            ),
            (
                "delete_user: loner, own id",
                loner,
                Action::DeleteUser { user_id: 4 },
                Forbidden,
            ),
            (
                "delete_user: loner, another user",
                loner,
                Action::DeleteUser { user_id: 2 },
                Hidden,
            ),
            // ListTeams, CreateTeam
            ("list_teams: lead", lead, Action::ListTeams, Allow),
            ("list_teams: member", member, Action::ListTeams, Allow),
            ("list_teams: loner", loner, Action::ListTeams, Allow),
            ("create_team: lead", lead, Action::CreateTeam, Forbidden),
            ("create_team: member", member, Action::CreateTeam, Forbidden),
            ("create_team: loner", loner, Action::CreateTeam, Forbidden),
            // ViewTeam
            (
                "view_team: lead, led team",
                lead,
                Action::ViewTeam { team_id: 10 },
                Allow,
            ),
            (
                "view_team: lead, team they only belong to",
                lead,
                Action::ViewTeam { team_id: 20 },
                Allow,
            ),
            (
                "view_team: lead, unrelated team",
                lead,
                Action::ViewTeam { team_id: 30 },
                Hidden,
            ),
            (
                "view_team: member, own team",
                member,
                Action::ViewTeam { team_id: 10 },
                Allow,
            ),
            (
                "view_team: member, unrelated team",
                member,
                Action::ViewTeam { team_id: 30 },
                Hidden,
            ),
            (
                "view_team: loner",
                loner,
                Action::ViewTeam { team_id: 10 },
                Hidden,
            ),
            // RenameTeam
            (
                "rename_team: lead, led team",
                lead,
                Action::RenameTeam { team_id: 10 },
                Allow,
            ),
            (
                "rename_team: lead, team they only belong to",
                lead,
                Action::RenameTeam { team_id: 20 },
                Forbidden,
            ),
            (
                "rename_team: lead, unrelated team",
                lead,
                Action::RenameTeam { team_id: 30 },
                Hidden,
            ),
            (
                "rename_team: member, own team",
                member,
                Action::RenameTeam { team_id: 10 },
                Forbidden,
            ),
            (
                "rename_team: member, unrelated team",
                member,
                Action::RenameTeam { team_id: 30 },
                Hidden,
            ),
            (
                "rename_team: loner",
                loner,
                Action::RenameTeam { team_id: 10 },
                Hidden,
            ),
            // DeleteTeam
            (
                "delete_team: lead, led team",
                lead,
                Action::DeleteTeam { team_id: 10 },
                Forbidden,
            ),
            (
                "delete_team: lead, team they only belong to",
                lead,
                Action::DeleteTeam { team_id: 20 },
                Forbidden,
            ),
            (
                "delete_team: lead, unrelated team",
                lead,
                Action::DeleteTeam { team_id: 30 },
                Hidden,
            ),
            (
                "delete_team: member, own team",
                member,
                Action::DeleteTeam { team_id: 10 },
                Forbidden,
            ),
            (
                "delete_team: member, unrelated team",
                member,
                Action::DeleteTeam { team_id: 30 },
                Hidden,
            ),
            (
                "delete_team: loner",
                loner,
                Action::DeleteTeam { team_id: 10 },
                Hidden,
            ),
            // AddMember
            ("add_member: lead, led team", lead, add(10), Allow),
            (
                "add_member: lead, team they only belong to",
                lead,
                add(20),
                Forbidden,
            ),
            ("add_member: lead, unrelated team", lead, add(30), Hidden),
            ("add_member: member, own team", member, add(10), Forbidden),
            (
                "add_member: member, unrelated team",
                member,
                add(30),
                Hidden,
            ),
            ("add_member: loner", loner, add(10), Hidden),
            // PutMember: admins only
            ("put_member: lead, led team", lead, put(10), Forbidden),
            (
                "put_member: lead, team they only belong to",
                lead,
                put(20),
                Forbidden,
            ),
            ("put_member: lead, unrelated team", lead, put(30), Hidden),
            ("put_member: member, own team", member, put(10), Forbidden),
            (
                "put_member: member, unrelated team",
                member,
                put(30),
                Hidden,
            ),
            ("put_member: loner", loner, put(10), Hidden),
            // RemoveMember
            (
                "remove_member: lead, a member of the led team",
                lead,
                remove(10, 3, Some(TeamRole::Member)),
                Allow,
            ),
            (
                "remove_member: lead, another lead of the led team",
                lead,
                remove(10, 9, Some(TeamRole::Lead)),
                Forbidden,
            ),
            (
                "remove_member: lead, themselves as lead",
                lead,
                remove(10, 2, Some(TeamRole::Lead)),
                Allow,
            ),
            (
                "remove_member: lead, someone who is not in the team",
                lead,
                remove(10, 9, None),
                Allow,
            ),
            (
                "remove_member: lead, team they only belong to",
                lead,
                remove(20, 3, Some(TeamRole::Member)),
                Forbidden,
            ),
            (
                "remove_member: lead, unrelated team",
                lead,
                remove(30, 3, Some(TeamRole::Member)),
                Hidden,
            ),
            (
                "remove_member: member, own team, another member",
                member,
                remove(10, 5, Some(TeamRole::Member)),
                Forbidden,
            ),
            (
                "remove_member: member, own team, themselves",
                member,
                remove(10, 3, Some(TeamRole::Member)),
                Forbidden,
            ),
            (
                "remove_member: member, unrelated team",
                member,
                remove(30, 5, Some(TeamRole::Member)),
                Hidden,
            ),
            (
                "remove_member: loner",
                loner,
                remove(10, 3, Some(TeamRole::Member)),
                Hidden,
            ),
            // ListKeys
            ("list_keys: lead", lead, Action::ListKeys, Allow),
            ("list_keys: member", member, Action::ListKeys, Allow),
            ("list_keys: loner", loner, Action::ListKeys, Allow),
            // CreateKey
            (
                "create_key: lead, own, no team",
                lead,
                create_key(2, None),
                Allow,
            ),
            (
                "create_key: lead, own, led team",
                lead,
                create_key(2, Some(10)),
                Allow,
            ),
            (
                "create_key: lead, own, team they only belong to",
                lead,
                create_key(2, Some(20)),
                Allow,
            ),
            (
                "create_key: lead, own, unrelated team",
                lead,
                create_key(2, Some(30)),
                Forbidden,
            ),
            (
                "create_key: lead, another user, led team",
                lead,
                create_key(3, Some(10)),
                Allow,
            ),
            (
                "create_key: lead, another user, team they only belong to",
                lead,
                create_key(3, Some(20)),
                Forbidden,
            ),
            (
                "create_key: lead, another user, unrelated team",
                lead,
                create_key(3, Some(30)),
                Forbidden,
            ),
            (
                "create_key: lead, another user, no team",
                lead,
                create_key(3, None),
                Forbidden,
            ),
            (
                "create_key: member, own, no team",
                member,
                create_key(3, None),
                Allow,
            ),
            (
                "create_key: member, own, own team",
                member,
                create_key(3, Some(10)),
                Allow,
            ),
            (
                "create_key: member, own, unrelated team",
                member,
                create_key(3, Some(30)),
                Forbidden,
            ),
            (
                "create_key: member, another user, own team",
                member,
                create_key(2, Some(10)),
                Forbidden,
            ),
            (
                "create_key: member, another user, no team",
                member,
                create_key(2, None),
                Forbidden,
            ),
            (
                "create_key: loner, own, no team",
                loner,
                create_key(4, None),
                Allow,
            ),
            (
                "create_key: loner, own, a team they are not in",
                loner,
                create_key(4, Some(10)),
                Forbidden,
            ),
            (
                "create_key: loner, another user, no team",
                loner,
                create_key(2, None),
                Forbidden,
            ),
            // ManageOwnTokens, ListProviders, ManageProviders, models, ViewAudit
            ("own_tokens: lead", lead, Action::ManageOwnTokens, Allow),
            ("own_tokens: member", member, Action::ManageOwnTokens, Allow),
            ("own_tokens: loner", loner, Action::ManageOwnTokens, Allow),
            ("list_providers: lead", lead, Action::ListProviders, Allow),
            (
                "list_providers: member",
                member,
                Action::ListProviders,
                Allow,
            ),
            ("list_providers: loner", loner, Action::ListProviders, Allow),
            (
                "manage_providers: lead",
                lead,
                Action::ManageProviders,
                Forbidden,
            ),
            (
                "manage_providers: member",
                member,
                Action::ManageProviders,
                Forbidden,
            ),
            (
                "manage_providers: loner",
                loner,
                Action::ManageProviders,
                Forbidden,
            ),
            ("list_models: lead", lead, Action::ListModels, Allow),
            ("list_models: member", member, Action::ListModels, Allow),
            ("list_models: loner", loner, Action::ListModels, Allow),
            ("manage_models: lead", lead, Action::ManageModels, Forbidden),
            (
                "manage_models: member",
                member,
                Action::ManageModels,
                Forbidden,
            ),
            (
                "manage_models: loner",
                loner,
                Action::ManageModels,
                Forbidden,
            ),
            ("list_routes: lead", lead, Action::ListRoutes, Allow),
            ("list_routes: member", member, Action::ListRoutes, Allow),
            ("list_routes: loner", loner, Action::ListRoutes, Allow),
            ("manage_routes: lead", lead, Action::ManageRoutes, Forbidden),
            (
                "manage_routes: member",
                member,
                Action::ManageRoutes,
                Forbidden,
            ),
            (
                "manage_routes: loner",
                loner,
                Action::ManageRoutes,
                Forbidden,
            ),
            ("view_audit: lead", lead, Action::ViewAudit, Forbidden),
            ("view_audit: member", member, Action::ViewAudit, Forbidden),
            ("view_audit: loner", loner, Action::ViewAudit, Forbidden),
            (
                "view_routing_health: lead",
                lead,
                Action::ViewRoutingHealth,
                Forbidden,
            ),
            (
                "view_routing_health: member",
                member,
                Action::ViewRoutingHealth,
                Forbidden,
            ),
            (
                "view_routing_health: loner",
                loner,
                Action::ViewRoutingHealth,
                Forbidden,
            ),
            ("manage_alerts: lead", lead, Action::ManageAlerts, Forbidden),
            (
                "manage_alerts: member",
                member,
                Action::ManageAlerts,
                Forbidden,
            ),
            (
                "manage_alerts: loner",
                loner,
                Action::ManageAlerts,
                Forbidden,
            ),
            (
                "manage_guardrails: lead",
                lead,
                Action::ManageGuardrails,
                Forbidden,
            ),
            (
                "manage_guardrails: member",
                member,
                Action::ManageGuardrails,
                Forbidden,
            ),
            (
                "manage_guardrails: loner",
                loner,
                Action::ManageGuardrails,
                Forbidden,
            ),
        ];
        let manage = |created_by| Action::ManagePrompt { created_by };
        cases.extend([
            ("list_prompts: lead", lead, Action::ListPrompts, Allow),
            ("list_prompts: member", member, Action::ListPrompts, Allow),
            ("list_prompts: loner", loner, Action::ListPrompts, Allow),
            ("create_prompt: lead", lead, Action::CreatePrompt, Allow),
            (
                "create_prompt: member",
                member,
                Action::CreatePrompt,
                Forbidden,
            ),
            (
                "create_prompt: loner",
                loner,
                Action::CreatePrompt,
                Forbidden,
            ),
            ("manage_prompt: lead, own", lead, manage(Some(2)), Allow),
            (
                "manage_prompt: lead, another's",
                lead,
                manage(Some(1)),
                Forbidden,
            ),
            (
                "manage_prompt: lead, nobody's",
                lead,
                manage(None),
                Forbidden,
            ),
            (
                "manage_prompt: member, own",
                member,
                manage(Some(3)),
                Forbidden,
            ),
            (
                "manage_prompt: loner, own",
                loner,
                manage(Some(4)),
                Forbidden,
            ),
        ]);
        let view_log = |user_id, team_id, user_in_led_team| Action::ViewLog {
            user_id,
            team_id,
            user_in_led_team,
        };
        cases.extend([
            ("list_logs: lead", lead, Action::ListLogs, Allow),
            ("list_logs: member", member, Action::ListLogs, Allow),
            ("list_logs: loner", loner, Action::ListLogs, Allow),
            ("list_limits: lead", lead, Action::ListLimits, Allow),
            ("list_limits: member", member, Action::ListLimits, Allow),
            ("list_limits: loner", loner, Action::ListLimits, Allow),
            ("manage_limits: lead", lead, Action::ManageLimits, Forbidden),
            (
                "manage_limits: member",
                member,
                Action::ManageLimits,
                Forbidden,
            ),
            (
                "manage_limits: loner",
                loner,
                Action::ManageLimits,
                Forbidden,
            ),
            ("list_budgets: lead", lead, Action::ListBudgets, Allow),
            ("list_budgets: member", member, Action::ListBudgets, Allow),
            ("list_budgets: loner", loner, Action::ListBudgets, Allow),
            (
                "manage_budgets: lead",
                lead,
                Action::ManageBudgets,
                Forbidden,
            ),
            (
                "manage_budgets: member",
                member,
                Action::ManageBudgets,
                Forbidden,
            ),
            (
                "manage_budgets: loner",
                loner,
                Action::ManageBudgets,
                Forbidden,
            ),
            ("list_usage: lead", lead, Action::ListUsage, Allow),
            ("list_usage: member", member, Action::ListUsage, Allow),
            ("list_usage: loner", loner, Action::ListUsage, Allow),
            ("use_playground: lead", lead, Action::UsePlayground, Allow),
            (
                "use_playground: member",
                member,
                Action::UsePlayground,
                Allow,
            ),
            ("use_playground: loner", loner, Action::UsePlayground, Allow),
            (
                "view_log: lead, own row",
                lead,
                view_log(Some(2), None, false),
                Allow,
            ),
            (
                "view_log: lead, led team's row",
                lead,
                view_log(Some(9), Some(10), false),
                Allow,
            ),
            (
                "view_log: lead, no-user row of led team",
                lead,
                view_log(None, Some(10), false),
                Allow,
            ),
            (
                "view_log: lead, row of a member of a led team",
                lead,
                view_log(Some(3), None, true),
                Allow,
            ),
            (
                "view_log: lead, team they only belong to",
                lead,
                view_log(Some(9), Some(20), false),
                Hidden,
            ),
            (
                "view_log: lead, row without user or team",
                lead,
                view_log(None, None, false),
                Hidden,
            ),
            (
                "view_log: member, own row",
                member,
                view_log(Some(3), Some(10), false),
                Allow,
            ),
            (
                "view_log: member, team row of another user",
                member,
                view_log(Some(2), Some(10), false),
                Hidden,
            ),
            (
                "view_log: member, shared-team flag does not count",
                member,
                view_log(Some(2), None, true),
                Hidden,
            ),
            (
                "view_log: loner, own row",
                loner,
                view_log(Some(4), None, false),
                Allow,
            ),
            (
                "view_log: loner, other row",
                loner,
                view_log(Some(3), None, false),
                Hidden,
            ),
        ]);
        cases.extend(key_cases(f, true, |owner_id, team_id| Action::ViewKey {
            owner_id,
            team_id,
        }));
        cases.extend(key_cases(f, false, |owner_id, team_id| Action::RevokeKey {
            owner_id,
            team_id,
        }));
        cases
    }

    #[test]
    fn policy_matrix() {
        let f = fixture();
        let cases = cases(&f);
        assert!(cases.len() >= 70, "only {} cases", cases.len());
        for (name, p, action, want) in &cases {
            assert_eq!(authorize(p, action), *want, "case: {name}");
        }
    }

    #[test]
    fn admin_is_always_allowed() {
        let f = fixture();
        for (name, _, action, _) in &cases(&f) {
            assert_eq!(authorize(&f.admin, action), Allow, "admin, case: {name}");
        }
        // Targets that are the admin's own, which the matrix does not reach.
        for action in [
            Action::ViewUser {
                user_id: 1,
                shares_led_team: false,
            },
            Action::UpdateUser {
                user_id: 1,
                changes_role_or_status: true,
            },
            Action::DeleteUser { user_id: 1 },
            Action::CreateKey {
                owner_id: 1,
                team_id: None,
            },
        ] {
            assert_eq!(authorize(&f.admin, &action), Allow, "admin, {action:?}");
        }
    }

    #[test]
    fn only_an_admin_edits_the_tags_of_a_key() {
        let f = fixture();
        let edit = |owner_id, team_id| Action::EditKeyTags { owner_id, team_id };
        for (owner, team) in [(Some(1), None), (Some(2), Some(10)), (None, None)] {
            assert_eq!(authorize(&f.admin, &edit(owner, team)), Allow);
        }
        // Who may see the key is told it is not theirs to edit; the rest, that it is not there.
        let own = Some(f.lead.user_id);
        assert_eq!(authorize(&f.lead, &edit(own, None)), Forbidden);
        assert_eq!(
            authorize(&f.member, &edit(Some(f.member.user_id), None)),
            Forbidden
        );
        assert_eq!(authorize(&f.lead, &edit(Some(99), Some(10))), Forbidden);
        assert_eq!(authorize(&f.lead, &edit(Some(99), Some(77))), Hidden);
        assert_eq!(authorize(&f.member, &edit(Some(99), None)), Hidden);
        assert_eq!(authorize(&f.loner, &edit(None, None)), Hidden);
    }

    #[test]
    fn nobody_else_manages_providers_or_audit() {
        let f = fixture();
        for p in [&f.lead, &f.member, &f.loner] {
            for action in [
                Action::ManageProviders,
                Action::ManageModels,
                Action::ManageRoutes,
                Action::ViewAudit,
                Action::ViewRoutingHealth,
                Action::ManageAlerts,
                Action::ManageGuardrails,
                Action::InviteUser { role: Role::Member },
                Action::InviteUser { role: Role::Admin },
                Action::CreateTeam,
            ] {
                assert_eq!(
                    authorize(p, &action),
                    Forbidden,
                    "user {}, {action:?}",
                    p.user_id
                );
            }
        }
    }

    #[test]
    fn list_scope_by_role() {
        let f = fixture();
        assert_eq!(list_scope(&f.admin), Scope::All);
        assert_eq!(
            list_scope(&f.lead),
            Scope::Teams {
                team_ids: vec![10],
                own_user_id: 2
            }
        );
        assert_eq!(list_scope(&f.member), Scope::Own { user_id: 3 });
        assert_eq!(list_scope(&f.loner), Scope::Own { user_id: 4 });
    }

    #[test]
    fn limits_that_apply_to_a_caller() {
        let f = fixture();
        // lead (2): leads 10, member of 20. member (3): member of 10.
        let access = |p: &Principal, scope, id, owner| limit_access(p, scope, id, owner);
        for p in [&f.admin, &f.lead, &f.member, &f.loner] {
            let want = if p.is_admin() { Spent } else { Figures };
            assert_eq!(access(p, LimitScope::Gateway, None, None), want);
        }
        assert_eq!(access(&f.admin, LimitScope::Team, Some(99), None), Spent);
        assert_eq!(access(&f.admin, LimitScope::Key, Some(99), Some(77)), Spent);
        // Their teams in any role, with the figures spent only for a lead.
        assert_eq!(access(&f.lead, LimitScope::Team, Some(10), None), Spent);
        assert_eq!(access(&f.lead, LimitScope::Team, Some(20), None), Figures);
        assert_eq!(access(&f.lead, LimitScope::Team, Some(30), None), Hide);
        assert_eq!(access(&f.member, LimitScope::Team, Some(10), None), Figures);
        assert_eq!(access(&f.member, LimitScope::Team, Some(20), None), Hide);
        assert_eq!(access(&f.loner, LimitScope::Team, Some(10), None), Hide);
        // Themselves, with the figures spent; not their team's members.
        assert_eq!(access(&f.member, LimitScope::User, Some(3), None), Spent);
        assert_eq!(access(&f.lead, LimitScope::User, Some(3), None), Hide);
        // Their own keys, with the figures spent; no one else's, not even
        // those of the people they lead or of a team they are in.
        assert_eq!(access(&f.member, LimitScope::Key, Some(5), Some(3)), Spent);
        assert_eq!(access(&f.lead, LimitScope::Key, Some(5), Some(3)), Hide);
        assert_eq!(access(&f.member, LimitScope::Key, Some(5), None), Hide);
        assert_eq!(access(&f.member, LimitScope::Key, Some(5), Some(77)), Hide);
        assert_eq!(access(&f.loner, LimitScope::Key, Some(5), Some(77)), Hide);
    }

    #[test]
    fn team_keys() {
        let f = fixture();
        // (who, owner, is a team key)
        for (who, p, owner, team_key) in [
            ("admin, for another", &f.admin, 3, false),
            ("admin, own", &f.admin, 1, false),
            ("lead, for a member", &f.lead, 3, true),
            ("lead, own", &f.lead, 2, false),
            ("member, own", &f.member, 3, false),
            ("member, for another", &f.member, 2, true),
        ] {
            assert_eq!(key_for_another_is_team_key(p, owner), team_key, "{who}");
        }
        // (owner's role, role in the team, may own a team key)
        for (role, team_role, may) in [
            (Role::Member, Some(TeamRole::Member), true),
            (Role::Member, Some(TeamRole::Lead), false),
            (Role::Member, None, false),
            (Role::Admin, Some(TeamRole::Member), false),
            (Role::Admin, Some(TeamRole::Lead), false),
            (Role::Admin, None, false),
        ] {
            assert_eq!(
                may_own_team_key(role, team_role),
                may,
                "{role:?} {team_role:?}"
            );
        }
    }
}
