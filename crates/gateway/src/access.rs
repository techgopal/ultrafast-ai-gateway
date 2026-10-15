//! Who may call which model or route on `/v1`.
//!
//! A call is allowed when the model is enabled, it is granted to the owner
//! of the key (to everyone, to one of their teams or to them; admins need
//! no grant), and the allowlist of the key, if it has one, names it. A key
//! without an owner can call only what is granted to everyone. Through a
//! route the allowlist names the route and the route must be open to the
//! owner; the model is still checked for enabled and granted.
//!
//! A key that a lead made for another user (a delegated key) acts for its
//! team only: it calls what is granted to everyone or to the key's team,
//! never its owner's own grants or an admin's reach, and nothing once its
//! owner is out of that team.

use std::collections::HashSet;

use crate::identity::Role;
use crate::snapshot::{SnapKey, SnapMcpServer, SnapMcpTool, SnapModel, SnapRoute, Snapshot};

pub enum Resolved<'a> {
    Model(&'a SnapModel),
    Route(&'a SnapRoute),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// No such model or route.
    Unknown,
    Forbidden,
}

fn allowlisted(key: &SnapKey, name: &str) -> bool {
    key.allowed
        .as_ref()
        .is_none_or(|names| names.contains(name))
}

/// Who is asking, as plain data. The `/v1` path builds it from the owner of
/// a key and the snapshot; the console paths from the signed-in user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer<'a> {
    /// No owner (a key made by the CLI): only what is for everyone.
    Nobody,
    /// An owner the snapshot does not hold. It acts for no one.
    Missing,
    User {
        id: i64,
        admin: bool,
        team_ids: &'a [i64],
    },
    /// A delegated key whose owner is in its team: only what is granted to
    /// everyone or to that team.
    Team { team_id: i64 },
}

/// What decides whether a model may be called, as plain data.
#[derive(Debug, Clone, Copy)]
pub struct ModelFacts<'a> {
    pub enabled: bool,
    /// Whether the provider of the model is usable.
    pub provider_present: bool,
    pub everyone: bool,
    pub team_ids: &'a [i64],
    pub user_ids: &'a [i64],
}

/// What decides whether a route may be used, as plain data.
#[derive(Debug, Clone, Copy)]
pub struct RouteFacts<'a> {
    pub everyone: bool,
    pub team_ids: &'a [i64],
}

/// Whether the model is enabled and granted to the viewer (to everyone, to
/// one of their teams or to them; admins need no grant). The one rule for
/// `/v1` and for what the console lists and lets a key name.
pub fn model_callable(viewer: Viewer<'_>, model: &ModelFacts<'_>) -> bool {
    if !model.enabled || !model.provider_present {
        return false;
    }
    match viewer {
        Viewer::Missing => false,
        Viewer::Nobody => model.everyone,
        Viewer::Team { team_id } => model.everyone || model.team_ids.contains(&team_id),
        Viewer::User {
            id,
            admin,
            team_ids,
        } => {
            model.everyone
                || admin
                || model.user_ids.contains(&id)
                || team_ids.iter().any(|t| model.team_ids.contains(t))
        }
    }
}

/// What decides whether an MCP tool may be called, as plain data.
#[derive(Debug, Clone, Copy)]
pub struct ToolFacts<'a> {
    pub enabled: bool,
    /// Whether its server is switched on and usable.
    pub server_present: bool,
    pub everyone: bool,
    pub team_ids: &'a [i64],
    pub user_ids: &'a [i64],
}

/// Whether the tool is enabled, its server present and the tool granted to
/// the viewer: exactly the rule of [`model_callable`].
pub fn tool_callable(viewer: Viewer<'_>, tool: &ToolFacts<'_>) -> bool {
    model_callable(
        viewer,
        &ModelFacts {
            enabled: tool.enabled,
            provider_present: tool.server_present,
            everyone: tool.everyone,
            team_ids: tool.team_ids,
            user_ids: tool.user_ids,
        },
    )
}

fn tool_facts<'a>(server: &SnapMcpServer, tool: &'a SnapMcpTool) -> ToolFacts<'a> {
    ToolFacts {
        // The snapshot holds only tools that are on.
        enabled: true,
        server_present: server.enabled && server.usable,
        everyone: tool.everyone,
        team_ids: &tool.team_ids,
        user_ids: &tool.user_ids,
    }
}

/// Whether the viewer may call the tool of the snapshot. The allowlist of a
/// key is not looked at here.
pub fn may_call_tool(viewer: Viewer<'_>, server: &SnapMcpServer, tool: &SnapMcpTool) -> bool {
    tool_callable(viewer, &tool_facts(server, tool))
}

/// Every tool the viewer may call, by server and tool name. `allowed` is
/// the allowlist of a key, if it has one: it must name `mcp:<server>/<tool>`.
pub fn callable_tools<'a>(
    snapshot: &'a Snapshot,
    viewer: Viewer<'_>,
    allowed: Option<&HashSet<String>>,
) -> Vec<(&'a SnapMcpServer, &'a SnapMcpTool)> {
    let mut out = Vec::new();
    for server in snapshot.mcp_servers() {
        for tool in snapshot.mcp_tools_of(server.id) {
            let named = allowed.is_none_or(|names| {
                names.contains(&crate::mcp::allow_entry(&server.name, &tool.name))
            });
            if named && may_call_tool(viewer, server, tool) {
                out.push((server, tool));
            }
        }
    }
    out
}

/// Whether the key may call this tool of the snapshot: its owner's reach,
/// cut by its allowlist. One tool, no scan of the registry.
pub fn key_may_call_tool(
    snapshot: &Snapshot,
    key: &SnapKey,
    server: &SnapMcpServer,
    tool: &SnapMcpTool,
) -> bool {
    key.allowed
        .as_ref()
        .is_none_or(|names| names.contains(&crate::mcp::allow_entry(&server.name, &tool.name)))
        && may_call_tool(viewer(snapshot, key), server, tool)
}

/// [`callable_tools`] for a virtual key: its owner's reach, cut by its allowlist.
pub fn callable_tools_of_key<'a>(
    snapshot: &'a Snapshot,
    key: &SnapKey,
) -> Vec<(&'a SnapMcpServer, &'a SnapMcpTool)> {
    callable_tools(snapshot, viewer(snapshot, key), key.allowed.as_ref())
}

/// Whether the route is open to the viewer: to everyone, to one of their
/// teams; admins always.
pub fn route_usable(viewer: Viewer<'_>, route: &RouteFacts<'_>) -> bool {
    match viewer {
        Viewer::Missing => false,
        Viewer::Nobody => route.everyone,
        Viewer::Team { team_id } => route.everyone || route.team_ids.contains(&team_id),
        Viewer::User {
            admin, team_ids, ..
        } => route.everyone || admin || team_ids.iter().any(|t| route.team_ids.contains(t)),
    }
}

fn viewer<'a>(snapshot: &'a Snapshot, key: &SnapKey) -> Viewer<'a> {
    if key.team_only {
        // Its owner must still be in its team; otherwise it acts for no one.
        let owner = key.user_id.and_then(|id| snapshot.user(id));
        return match (owner, key.team_id) {
            (Some(user), Some(team_id)) if user.team_ids.contains(&team_id) => {
                Viewer::Team { team_id }
            }
            _ => Viewer::Missing,
        };
    }
    match key.user_id {
        None => Viewer::Nobody,
        Some(id) => snapshot
            .user(id)
            .map_or(Viewer::Missing, |user| Viewer::User {
                id,
                admin: user.role == Role::Admin,
                team_ids: &user.team_ids,
            }),
    }
}

fn model_facts(model: &SnapModel) -> ModelFacts<'_> {
    ModelFacts {
        enabled: model.enabled,
        // The snapshot holds only models of usable providers.
        provider_present: true,
        everyone: model.everyone,
        team_ids: &model.team_ids,
        user_ids: &model.user_ids,
    }
}

/// Whether the model is enabled and granted to the owner of the key. The
/// allowlist is not looked at: the caller checks it, by the model name for
/// a direct call and by the route name for a call through a route.
pub fn may_call_model(snapshot: &Snapshot, key: &SnapKey, model: &SnapModel) -> bool {
    model_callable(viewer(snapshot, key), &model_facts(model))
}

fn may_use_route(snapshot: &Snapshot, key: &SnapKey, route: &SnapRoute) -> bool {
    route_usable(
        viewer(snapshot, key),
        &RouteFacts {
            everyone: route.everyone,
            team_ids: &route.team_ids,
        },
    )
}

/// The targets of a route that this key may call, in order: primaries,
/// then fallbacks.
fn route_targets<'a>(
    snapshot: &'a Snapshot,
    key: &'a SnapKey,
    route: &'a SnapRoute,
) -> impl Iterator<Item = &'a SnapModel> {
    route
        .targets()
        .filter_map(|t| snapshot.model(&t.provider, &t.model))
        .filter(|m| may_call_model(snapshot, key, m))
}

/// What `requested` names, and whether this key may call it. A name with a
/// `/` is `provider/model`, split at the first `/` only; a name without one
/// is a route.
pub fn resolve<'a>(
    snapshot: &'a Snapshot,
    key: &'a SnapKey,
    requested: &str,
) -> Result<Resolved<'a>, Denied> {
    if let Some((provider, name)) = requested.split_once('/') {
        let model = snapshot.model(provider, name).ok_or(Denied::Unknown)?;
        return if may_call_model(snapshot, key, model) && allowlisted(key, requested) {
            Ok(Resolved::Model(model))
        } else {
            Err(Denied::Forbidden)
        };
    }
    let route = snapshot.route(requested).ok_or(Denied::Unknown)?;
    if !may_use_route(snapshot, key, route) || !allowlisted(key, &route.name) {
        return Err(Denied::Forbidden);
    }
    // A route with nothing left to call, or with nothing enabled, is not the
    // caller's to be refused: the call fails as unavailable. Refused is a
    // route with an enabled model that this key may not call.
    let any_enabled = route
        .targets()
        .filter_map(|t| snapshot.model(&t.provider, &t.model))
        .any(|m| m.enabled);
    if !any_enabled || route_targets(snapshot, key, route).next().is_some() {
        Ok(Resolved::Route(route))
    } else {
        Err(Denied::Forbidden)
    }
}

/// `(id, owned_by)` of everything this key can call, sorted by id.
pub fn callable_names(snapshot: &Snapshot, key: &SnapKey) -> Vec<(String, String)> {
    let models = snapshot
        .models()
        .map(|m| (format!("{}/{}", m.provider, m.name), m.provider.clone()));
    let routes = snapshot
        .routes()
        .map(|r| (r.name.clone(), "ultrafast-route".to_string()));
    let mut names: Vec<(String, String)> = models
        .chain(routes)
        .filter(|(id, _)| match resolve(snapshot, key, id) {
            // A route with nothing callable would answer 503: it is not listed.
            Ok(Resolved::Route(route)) => route_targets(snapshot, key, route).next().is_some(),
            Ok(Resolved::Model(_)) => true,
            Err(_) => false,
        })
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model<'a>(everyone: bool, team_ids: &'a [i64], user_ids: &'a [i64]) -> ModelFacts<'a> {
        ModelFacts {
            enabled: true,
            provider_present: true,
            everyone,
            team_ids,
            user_ids,
        }
    }

    #[test]
    fn a_team_viewer_has_what_is_everyones_or_its_teams() {
        let team = Viewer::Team { team_id: 10 };
        assert!(model_callable(team, &model(true, &[], &[])));
        assert!(model_callable(team, &model(false, &[10], &[])));
        assert!(!model_callable(team, &model(false, &[20], &[])));
        // Never a user's own grant, whoever the user is.
        assert!(!model_callable(team, &model(false, &[], &[1, 2, 3])));
        // Never what only admins reach.
        assert!(!model_callable(team, &model(false, &[], &[])));
        let mut off = model(true, &[10], &[]);
        off.enabled = false;
        assert!(!model_callable(team, &off));

        let route = |everyone, team_ids| RouteFacts { everyone, team_ids };
        assert!(route_usable(team, &route(true, &[])));
        assert!(route_usable(team, &route(false, &[10])));
        assert!(!route_usable(team, &route(false, &[20])));
        assert!(!route_usable(team, &route(false, &[])));
    }

    fn tool<'a>(everyone: bool, team_ids: &'a [i64], user_ids: &'a [i64]) -> ToolFacts<'a> {
        ToolFacts {
            enabled: true,
            server_present: true,
            everyone,
            team_ids,
            user_ids,
        }
    }

    fn user<'a>(id: i64, admin: bool, team_ids: &'a [i64]) -> Viewer<'a> {
        Viewer::User {
            id,
            admin,
            team_ids,
        }
    }

    /// A tool is granted exactly like a model: the same viewers get the same
    /// answers for the same facts.
    #[test]
    fn a_tool_follows_the_model_rule_for_every_viewer() {
        let teams = [10];
        let viewers = [
            Viewer::Nobody,
            Viewer::Missing,
            user(1, false, &teams),
            user(1, false, &[]),
            user(2, true, &[]),
            Viewer::Team { team_id: 10 },
            Viewer::Team { team_id: 20 },
        ];
        let grants: [(bool, &[i64], &[i64]); 5] = [
            (true, &[], &[]),
            (false, &[10], &[]),
            (false, &[], &[1]),
            (false, &[20], &[2]),
            (false, &[], &[]),
        ];
        for viewer in viewers {
            for (everyone, t, u) in grants {
                for (enabled, present) in [(true, true), (false, true), (true, false)] {
                    let mut facts = tool(everyone, t, u);
                    facts.enabled = enabled;
                    facts.server_present = present;
                    let as_model = ModelFacts {
                        enabled,
                        provider_present: present,
                        everyone,
                        team_ids: t,
                        user_ids: u,
                    };
                    assert_eq!(
                        tool_callable(viewer, &facts),
                        model_callable(viewer, &as_model),
                        "{viewer:?} {facts:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_tool_is_not_callable_when_off_or_its_server_is_gone() {
        let mut facts = tool(true, &[], &[]);
        assert!(tool_callable(user(1, true, &[]), &facts));
        facts.enabled = false;
        assert!(!tool_callable(user(1, true, &[]), &facts));
        facts.enabled = true;
        facts.server_present = false;
        assert!(!tool_callable(user(1, true, &[]), &facts));
        // Admins need no grant; others need one.
        let facts = tool(false, &[], &[]);
        assert!(tool_callable(user(1, true, &[]), &facts));
        assert!(!tool_callable(user(1, false, &[10]), &facts));
        assert!(!tool_callable(Viewer::Team { team_id: 10 }, &facts));
    }
}
