//! `ultrafast model add`: the catalog from the command line.

mod common;

use common::api;
use ultrafast_gateway::catalog::{add_model, describe_model_add, ModelAdded};

#[tokio::test]
async fn model_add_creates_enables_grants_and_is_audited_as_the_cli() {
    let api = api().await;
    let store = &api.store;
    store
        .insert_provider("p", "openai", "https://x.example.com", None)
        .await
        .unwrap();

    // Missing: created, enabled, granted to everyone.
    let out = add_model(store, "p", "gpt-4o", true, true).await.unwrap();
    assert_eq!(out, ModelAdded { created: true });
    let rows = store.list_models().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].enabled);
    let grants = store.grants_of(rows[0].id).await.unwrap();
    assert!(grants.everyone);

    // Again: nothing new, same result.
    let out = add_model(store, "p", "gpt-4o", true, true).await.unwrap();
    assert_eq!(out, ModelAdded { created: false });
    assert_eq!(store.list_models().await.unwrap().len(), 1);

    // Without the flags: added disabled, granted to nobody.
    add_model(store, "p", "gpt-4o-mini", false, false)
        .await
        .unwrap();
    let rows = store.list_models().await.unwrap();
    let mini = rows.iter().find(|m| m.name == "gpt-4o-mini").unwrap();
    assert!(!mini.enabled);
    assert_eq!(store.grants_of(mini.id).await.unwrap(), Default::default());

    // The same validation as the API, and a provider that must exist.
    assert!(add_model(store, "p", "has space", true, true)
        .await
        .is_err());
    assert!(add_model(store, "p", "", true, true).await.is_err());
    assert!(add_model(store, "nope", "m", true, true).await.is_err());

    let audit = store.list_audit(50, None).await.unwrap();
    let entry = audit.iter().find(|r| r.action == "model.add").unwrap();
    assert_eq!(entry.actor_email, "cli");
}

#[test]
fn model_add_says_what_happened_and_when_it_takes_effect() {
    let created = ModelAdded { created: true };
    let existing = ModelAdded { created: false };
    assert_eq!(
        describe_model_add(&created, "p", "m", true, true),
        "Added model 'm' of 'p', enabled, granted to everyone. A running gateway picks this up within 30 seconds."
    );
    assert_eq!(
        describe_model_add(&created, "p", "m", false, false),
        "Added model 'm' of 'p'. A running gateway picks this up within 30 seconds."
    );
    assert_eq!(
        describe_model_add(&existing, "p", "m", true, false),
        "Updated model 'm' of 'p', enabled. A running gateway picks this up within 30 seconds."
    );
    // Nothing changed: no flags and already there.
    assert_eq!(
        describe_model_add(&existing, "p", "m", false, false),
        "Model p/m is already in the catalog."
    );
}
