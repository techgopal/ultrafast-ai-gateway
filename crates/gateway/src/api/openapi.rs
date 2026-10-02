//! The OpenAPI description of `/api`, built from the annotations on the
//! handlers. `/v1` is not part of it.
//!
//! The handlers build most bodies with `json!`. The types here give those
//! bodies a schema; nothing at runtime uses them.

use std::collections::BTreeMap;

use utoipa::openapi::path::{Operation, Parameter, ParameterBuilder, ParameterIn};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::openapi::Required;
use utoipa::{Modify, OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;

use super::auth::{UserTeamView, UserView};
use super::keys::KeyView;
use super::logs::LogView;
use super::providers::ProviderView;
use super::tokens::TokenView;
use super::usage::UsageRow;
use super::{CSRF_HEADER, SESSION_COOKIE};
use crate::routing::TargetHealth;
use crate::store::{AuditRow, MemberDetail, TeamSummary};

/// Every error of `/api` has this shape.
#[derive(ToSchema)]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}

#[derive(ToSchema)]
pub struct ApiErrorDetail {
    /// A stable name for the error, such as `not_found`.
    pub code: String,
    /// Text for a person. It may change.
    pub message: String,
    /// For `validation_failed` only: a message for each field that is not
    /// valid.
    #[schema(nullable = false)]
    pub fields: Option<BTreeMap<String, String>>,
}

#[derive(ToSchema)]
pub struct SetupStatus {
    /// True while no user exists.
    pub needs_setup: bool,
}

#[derive(ToSchema)]
pub struct LoginResponse {
    pub user: UserView,
    /// Send it as the `x-csrf-token` header with every request of this
    /// session that is not a GET.
    pub csrf_token: String,
}

#[derive(ToSchema)]
pub struct MeResponse {
    pub user: UserView,
    pub teams: Vec<UserTeamView>,
    /// The CSRF token of the session. `null` for a caller with an access
    /// token.
    #[schema(required)]
    pub csrf_token: Option<String>,
}

#[derive(ToSchema)]
pub struct UserList {
    pub users: Vec<UserView>,
}

#[derive(ToSchema)]
pub struct InviteResponse {
    pub user: UserView,
    /// The link that lets the user set a password. It is shown once, in
    /// this answer, and cannot be read again.
    pub invite_link: String,
}

#[derive(ToSchema)]
pub struct ReinviteResponse {
    /// The link that lets the user set a password. It is shown once, in
    /// this answer, and cannot be read again.
    pub invite_link: String,
}

#[derive(ToSchema)]
pub struct TeamList {
    pub teams: Vec<TeamSummary>,
}

#[derive(ToSchema)]
pub struct TeamDetail {
    pub team: TeamSummary,
    pub members: Vec<MemberDetail>,
}

#[derive(ToSchema)]
pub struct KeyList {
    pub keys: Vec<KeyView>,
}

#[derive(ToSchema)]
pub struct CreatedKey {
    pub key: KeyView,
    /// The virtual key itself. It is shown once, in this answer, and
    /// cannot be read again.
    pub secret: String,
}

#[derive(ToSchema)]
pub struct ProviderList {
    pub providers: Vec<ProviderView>,
}

#[derive(ToSchema)]
pub struct TokenList {
    pub tokens: Vec<TokenView>,
}

#[derive(ToSchema)]
pub struct CreatedToken {
    pub token: TokenView,
    /// The access token itself. It is shown once, in this answer, and
    /// cannot be read again.
    pub secret: String,
}

#[derive(ToSchema)]
pub struct AuditPage {
    pub entries: Vec<AuditRow>,
}

#[derive(ToSchema)]
pub struct LogPage {
    pub logs: Vec<LogView>,
}

#[derive(ToSchema)]
pub struct UsagePage {
    /// First day of the range, `YYYY-MM-DD`, UTC.
    pub from: String,
    /// Last day of the range, `YYYY-MM-DD`, UTC.
    pub to: String,
    /// The sums over every group.
    pub total: UsageRow,
    pub rows: Vec<UsageRow>,
}

#[derive(ToSchema)]
pub struct RoutingHealth {
    pub targets: Vec<TargetHealth>,
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ultrafast Gateway Admin API",
        description = "The admin API of the Ultrafast gateway, served under /api."
    ),
    tags(
        (name = "auth", description = "Setup, sign-in and the caller's own account."),
        (name = "users", description = "Users and their invites."),
        (name = "teams", description = "Teams and their members."),
        (name = "keys", description = "Virtual keys for /v1."),
        (name = "providers", description = "Upstream providers."),
        (name = "models", description = "The model catalog and who may call each model."),
        (name = "routes", description = "Routes: named sets of models with fallbacks and limits."),
        (name = "tokens", description = "The caller's access tokens for /api."),
        (name = "audit", description = "The audit log."),
        (name = "routing", description = "How calls are routed to providers."),
        (name = "settings", description = "Settings of the gateway."),
        (name = "limits", description = "Rate limits of /v1."),
        (name = "budgets", description = "Spending budgets of /v1."),
        (name = "logs", description = "Request logs."),
        (name = "usage", description = "Usage sums over the request logs."),
    )
)]
struct AdminApi;

/// Adds the two ways to authenticate, and the CSRF header to every
/// operation that needs a caller and is not a GET.
struct Credentials;

fn csrf_parameter() -> Parameter {
    ParameterBuilder::new()
        .name(CSRF_HEADER)
        .parameter_in(ParameterIn::Header)
        .required(Required::False)
        .description(Some(
            "The CSRF token of the session. Required with a session cookie; \
             not needed with an access token.",
        ))
        .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
        .build()
}

impl Modify for Credentials {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                SESSION_COOKIE,
                "The session cookie set by signing in.",
            ))),
        );
        components.add_security_scheme(
            "token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some("An access token, which starts with `uf-at-`."))
                    .build(),
            ),
        );

        let add_csrf = |operation: &mut Option<Operation>| {
            let Some(operation) = operation else { return };
            if operation.security.as_ref().is_some_and(|s| !s.is_empty()) {
                operation
                    .parameters
                    .get_or_insert_with(Vec::new)
                    .push(csrf_parameter().into());
            }
        };
        for item in openapi.paths.paths.values_mut() {
            add_csrf(&mut item.post);
            add_csrf(&mut item.put);
            add_csrf(&mut item.patch);
            add_csrf(&mut item.delete);
        }
    }
}

/// The description of `/api`, made from the routes of the router. It
/// reads nothing: not the data directory, not the database, not the
/// master key.
pub fn spec() -> utoipa::openapi::OpenApi {
    let (_, mut spec) = OpenApiRouter::with_openapi(AdminApi::openapi())
        .nest("/api", super::documented())
        .split_for_parts();
    // After the routes are in, as it adds to each operation.
    Credentials.modify(&mut spec);
    spec
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;

    /// Every route of `api::router`, which has 56. Its fallbacks are not
    /// routes.
    const ROUTES: [(&str, &str); 56] = [
        ("GET", "/api/setup"),
        ("POST", "/api/setup"),
        ("POST", "/api/auth/login"),
        ("POST", "/api/auth/logout"),
        ("GET", "/api/auth/me"),
        ("POST", "/api/auth/accept-invite"),
        ("POST", "/api/auth/password"),
        ("GET", "/api/users"),
        ("POST", "/api/users"),
        ("GET", "/api/users/{id}"),
        ("PATCH", "/api/users/{id}"),
        ("DELETE", "/api/users/{id}"),
        ("POST", "/api/users/{id}/invite"),
        ("GET", "/api/teams"),
        ("POST", "/api/teams"),
        ("GET", "/api/teams/{id}"),
        ("PATCH", "/api/teams/{id}"),
        ("DELETE", "/api/teams/{id}"),
        ("POST", "/api/teams/{id}/members"),
        ("PUT", "/api/teams/{id}/members/{user_id}"),
        ("DELETE", "/api/teams/{id}/members/{user_id}"),
        ("GET", "/api/keys"),
        ("POST", "/api/keys"),
        ("GET", "/api/keys/{id}"),
        ("DELETE", "/api/keys/{id}"),
        ("GET", "/api/providers"),
        ("POST", "/api/providers"),
        ("PATCH", "/api/providers/{id}"),
        ("DELETE", "/api/providers/{id}"),
        ("POST", "/api/providers/{id}/sync"),
        ("GET", "/api/models"),
        ("POST", "/api/models"),
        ("PATCH", "/api/models/{id}"),
        ("DELETE", "/api/models/{id}"),
        ("PUT", "/api/models/{id}/grants"),
        ("GET", "/api/routes"),
        ("POST", "/api/routes"),
        ("GET", "/api/routes/{id}"),
        ("PUT", "/api/routes/{id}"),
        ("DELETE", "/api/routes/{id}"),
        ("GET", "/api/tokens"),
        ("POST", "/api/tokens"),
        ("DELETE", "/api/tokens/{id}"),
        ("GET", "/api/audit"),
        ("GET", "/api/routing/health"),
        ("GET", "/api/settings"),
        ("PATCH", "/api/settings"),
        ("GET", "/api/limits"),
        ("PUT", "/api/limits"),
        ("DELETE", "/api/limits/{id}"),
        ("GET", "/api/budgets"),
        ("PUT", "/api/budgets"),
        ("DELETE", "/api/budgets/{id}"),
        ("GET", "/api/logs"),
        ("GET", "/api/logs/{id}"),
        ("GET", "/api/usage"),
    ];

    const SECRET_REQUEST_FIELDS: [&str; 5] = [
        "password",
        "current_password",
        "new_password",
        "api_key",
        "token",
    ];

    const NEVER_SHOWN: [&str; 5] = [
        "password_hash",
        "credential",
        "key_hash",
        "token_hash",
        "id_hash",
    ];

    fn spec_json() -> Value {
        serde_json::to_value(spec()).unwrap()
    }

    /// Every operation as (METHOD, path, operation).
    fn operations(spec: &Value) -> Vec<(String, String, &Value)> {
        let mut all = Vec::new();
        for (path, item) in spec["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                all.push((method.to_uppercase(), path.clone(), operation));
            }
        }
        all
    }

    /// The schema itself, for a schema or a reference to one.
    fn resolve<'a>(spec: &'a Value, schema: &'a Value) -> &'a Value {
        match schema["$ref"].as_str() {
            Some(reference) => {
                let name = reference
                    .strip_prefix("#/components/schemas/")
                    .expect("a reference to a component schema");
                let target = &spec["components"]["schemas"][name];
                assert!(target.is_object(), "{reference} does not exist");
                target
            }
            None => schema,
        }
    }

    /// The schema and every schema it contains, references followed.
    fn collect<'a>(spec: &'a Value, schema: &'a Value, seen: &mut Vec<&'a Value>) {
        let schema = resolve(spec, schema);
        if seen.iter().any(|s| std::ptr::eq(*s, schema)) {
            return;
        }
        seen.push(schema);
        if let Some(properties) = schema["properties"].as_object() {
            for property in properties.values() {
                collect(spec, property, seen);
            }
        }
        for key in ["items", "additionalProperties"] {
            if schema[key].is_object() {
                collect(spec, &schema[key], seen);
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            for part in schema[key].as_array().into_iter().flatten() {
                collect(spec, part, seen);
            }
        }
    }

    fn body_schemas(content: &Value) -> impl Iterator<Item = &Value> {
        content
            .as_object()
            .into_iter()
            .flat_map(|types| types.values())
            .map(|media| &media["schema"])
    }

    #[test]
    fn spec_lists_every_route() {
        let spec = spec_json();
        let documented: BTreeSet<(String, String)> = operations(&spec)
            .into_iter()
            .map(|(method, path, _)| (method, path))
            .collect();
        let routes: BTreeSet<(String, String)> = ROUTES
            .iter()
            .map(|(method, path)| (method.to_string(), path.to_string()))
            .collect();
        assert_eq!(routes.len(), 56);
        assert_eq!(documented, routes);
    }

    #[test]
    fn operation_ids_are_unique() {
        let spec = spec_json();
        let mut ids = BTreeSet::new();
        for (method, path, operation) in operations(&spec) {
            let id = operation["operationId"]
                .as_str()
                .unwrap_or_else(|| panic!("{method} {path} has no operationId"));
            let words: Vec<&str> = id.split('_').collect();
            assert!(
                words.len() >= 2
                    && words
                        .iter()
                        .all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase())),
                "{id} of {method} {path} is not of the form tag_action"
            );
            assert!(ids.insert(id.to_string()), "{id} names two operations");
        }
        assert_eq!(ids.len(), 56);
    }

    #[test]
    fn info_names_the_api_and_the_version() {
        let spec = spec_json();
        assert_eq!(spec["info"]["title"], "Ultrafast Gateway Admin API");
        assert_eq!(spec["info"]["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn secrets_are_write_only() {
        let spec = spec_json();
        let mut checked = BTreeSet::new();
        for (method, path, operation) in operations(&spec) {
            for body in body_schemas(&operation["requestBody"]["content"]) {
                let mut schemas = Vec::new();
                collect(&spec, body, &mut schemas);
                for schema in schemas {
                    let Some(properties) = schema["properties"].as_object() else {
                        continue;
                    };
                    for name in SECRET_REQUEST_FIELDS {
                        let Some(property) = properties.get(name) else {
                            continue;
                        };
                        assert_eq!(
                            property["writeOnly"], true,
                            "{name} of {method} {path} is not write-only"
                        );
                        checked.insert(name);
                    }
                }
            }
        }
        // Each of the names was found, so the test looked where they are.
        assert_eq!(checked, BTreeSet::from(SECRET_REQUEST_FIELDS));
    }

    #[test]
    fn no_response_schema_has_secret_fields() {
        let spec = spec_json();
        let mut schemas = Vec::new();
        for (_, _, operation) in operations(&spec) {
            for response in operation["responses"].as_object().unwrap().values() {
                for body in body_schemas(&response["content"]) {
                    collect(&spec, body, &mut schemas);
                }
            }
        }
        assert!(schemas.len() > 20, "only {} schemas", schemas.len());
        for schema in schemas {
            let Some(properties) = schema["properties"].as_object() else {
                continue;
            };
            for name in NEVER_SHOWN {
                assert!(!properties.contains_key(name), "{name} in {schema}");
            }
        }
    }

    #[test]
    fn every_error_is_the_shared_shape() {
        let spec = spec_json();
        for (method, path, operation) in operations(&spec) {
            for (status, response) in operation["responses"].as_object().unwrap() {
                if status.starts_with('2') {
                    continue;
                }
                let schema = &response["content"]["application/json"]["schema"];
                assert_eq!(
                    schema["$ref"], "#/components/schemas/ApiErrorBody",
                    "{status} of {method} {path}"
                );
            }
        }
    }

    #[test]
    fn credentials_and_csrf_are_declared() {
        let spec = spec_json();
        let schemes = &spec["components"]["securitySchemes"];
        assert_eq!(schemes["session"]["type"], "apiKey");
        assert_eq!(schemes["session"]["in"], "cookie");
        assert_eq!(schemes["session"]["name"], "uf_session");
        assert_eq!(schemes["token"]["type"], "http");
        assert_eq!(schemes["token"]["scheme"], "bearer");

        for (method, path, operation) in operations(&spec) {
            let needs_caller = operation["security"]
                .as_array()
                .is_some_and(|s| !s.is_empty());
            let csrf = operation["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["name"] == "x-csrf-token" && p["in"] == "header")
                .count();
            let expected = usize::from(needs_caller && method != "GET");
            assert_eq!(csrf, expected, "{method} {path}");
        }
    }

    #[test]
    fn path_parameters_are_declared_once() {
        let spec = spec_json();
        for (method, path, operation) in operations(&spec) {
            let declared: Vec<&str> = operation["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["in"] == "path")
                .map(|p| p["name"].as_str().unwrap())
                .collect();
            let in_path: Vec<&str> = path
                .split('/')
                .filter_map(|part| part.strip_prefix('{')?.strip_suffix('}'))
                .collect();
            assert_eq!(declared, in_path, "{method} {path}");
        }
    }

    #[test]
    fn spec_is_the_same_every_time() {
        let first = serde_json::to_string_pretty(&spec()).unwrap();
        let second = serde_json::to_string_pretty(&spec()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn committed_spec_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../openapi/admin.json");
        let committed = std::fs::read_to_string(path).expect("openapi/admin.json");
        let committed: Value = serde_json::from_str(&committed).unwrap();
        assert_eq!(
            spec_json(),
            committed,
            "openapi/admin.json is out of date; run: \
             cargo run -p ultrafast-gateway -- openapi > openapi/admin.json"
        );
    }
}
