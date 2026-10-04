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

use crate::identity::Role;
use crate::snapshot::{SnapKey, SnapModel, SnapRoute, Snapshot};

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
}
