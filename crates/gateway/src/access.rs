//! Who may call which model or route on `/v1`.
//!
//! A call is allowed when the model is enabled, it is granted to the owner
//! of the key (to everyone, to one of their teams or to them; admins need
//! no grant), and the allowlist of the key, if it has one, names it. A key
//! without an owner can call only what is granted to everyone. Through a
//! route the allowlist names the route and the route must be open to the
//! owner; the model is still checked for enabled and granted.

use crate::identity::Role;
use crate::snapshot::{SnapKey, SnapModel, SnapRoute, SnapUser, Snapshot};

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

/// Who a key acts for.
enum Owner<'a> {
    /// The key has no owner (made by the CLI).
    Nobody,
    Known(i64, &'a SnapUser),
    /// The key names an owner the snapshot does not hold. It acts for no
    /// one: it may call nothing.
    Missing,
}

fn owner<'a>(snapshot: &'a Snapshot, key: &SnapKey) -> Owner<'a> {
    match key.user_id {
        None => Owner::Nobody,
        Some(id) => snapshot
            .user(id)
            .map_or(Owner::Missing, |user| Owner::Known(id, user)),
    }
}

/// Whether the model is enabled and granted to the owner of the key. The
/// allowlist is not looked at: the caller checks it, by the model name for
/// a direct call and by the route name for a call through a route.
pub fn may_call_model(snapshot: &Snapshot, key: &SnapKey, model: &SnapModel) -> bool {
    if !model.enabled {
        return false;
    }
    match owner(snapshot, key) {
        Owner::Missing => false,
        Owner::Nobody => model.everyone,
        Owner::Known(id, user) => {
            model.everyone
                || user.role == Role::Admin
                || model.user_ids.contains(&id)
                || user.team_ids.iter().any(|t| model.team_ids.contains(t))
        }
    }
}

fn may_use_route(snapshot: &Snapshot, key: &SnapKey, route: &SnapRoute) -> bool {
    match owner(snapshot, key) {
        Owner::Missing => false,
        Owner::Nobody => route.everyone,
        Owner::Known(_, user) => {
            route.everyone
                || user.role == Role::Admin
                || user.team_ids.iter().any(|t| route.team_ids.contains(t))
        }
    }
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
    // A route whose models are all gone is not the caller's to be refused:
    // the call fails as unavailable.
    if !route.has_targets() || route_targets(snapshot, key, route).next().is_some() {
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
            // A route with no target left would answer 503: it is not listed.
            Ok(Resolved::Route(route)) => route.has_targets(),
            Ok(Resolved::Model(_)) => true,
            Err(_) => false,
        })
        .collect();
    names.sort();
    names
}
