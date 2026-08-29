use std::collections::BTreeMap;

use agentsassemble_domain::{
    PersonaAssetKind, PersonaCard, PersonaLoreEntry, PersonaLoreSettings, RoomInputDeliveryKind,
    RoomSettings, public_settings,
};
use serde_json::json;

use super::{AGENT_ID, fixture, save_stored_session, stored_session};
use crate::{ImportedPersonaAsset, SqliteStore};

#[tokio::test]
async fn ordered_and_ambient_persona_inputs_are_frozen_across_library_replacement() {
    for (mode, delivery_kind) in [
        ("ordered", RoomInputDeliveryKind::OrderedObservation),
        ("ambient", RoomInputDeliveryKind::AmbientObservation),
    ] {
        let (store, principal, _directory) = fixture().await;
        if mode == "ambient" {
            let revision = public_settings(&RoomSettings::defaults("General"))
                .unwrap_or_else(|error| panic!("default settings: {error}"))
                .settings_revision;
            store
                .execute_room_settings_update(
                    &principal,
                    "persona-ambient-settings",
                    &json!({"expected_revision": revision, "conversation_mode": "ambient"}),
                )
                .await
                .unwrap_or_else(|error| panic!("enable ambient mode: {error}"));
        }
        let summary = store_persona(&store, persona("old lantern rule")).await;
        let mut session = stored_session(&store).await;
        session.public.persona_card_id = "guide".into();
        session.public.persona_card = Some(Box::new(summary));
        save_stored_session(&store, &session).await;

        let assigned = store
            .execute_message_with_turn(
                &principal,
                "persona-turn",
                "message.send",
                &json!({"content": "@Terra the harbor is dark"}),
            )
            .await
            .unwrap_or_else(|error| panic!("assign {mode} persona turn: {error}"))
            .assignments
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{mode} persona message must be assigned"));
        assert_eq!(assigned.delivery_kind, delivery_kind);
        assert!(assigned.provider_input.contains("old lantern rule"));
        assert!(assigned.provider_input.contains("harbor lore"));
        assert!(assigned.provider_input.contains("the harbor is dark"));
        assert!(assigned.provider_input.contains(if mode == "ordered" {
            "[Ordered shared-room observation]"
        } else {
            "[Ambient shared-room observation]"
        }));

        store_persona(&store, persona("replacement compass rule")).await;
        let page = store
            .load_provider_turn_reconciliation_page(None)
            .await
            .unwrap_or_else(|error| panic!("load assigned {mode} turn: {error}"));
        let recovered = store
            .recover_assigned_provider_turn(
                page.candidates
                    .first()
                    .unwrap_or_else(|| panic!("assigned {mode} turn must be recoverable")),
            )
            .await
            .unwrap_or_else(|error| panic!("recover assigned {mode} turn: {error}"));
        assert_eq!(recovered.session.public.session_id, AGENT_ID);
        assert_eq!(recovered.provider_input, assigned.provider_input);
        assert!(
            !recovered
                .provider_input
                .contains("replacement compass rule")
        );
    }
}

async fn store_persona(
    store: &SqliteStore,
    card: PersonaCard,
) -> agentsassemble_domain::PersonaAssetSummary {
    store
        .replace_persona_asset(ImportedPersonaAsset {
            card,
            thumbnail: None,
        })
        .await
        .unwrap_or_else(|error| panic!("store persona: {error}"))
}

fn persona(system_prompt: &str) -> PersonaCard {
    PersonaCard {
        id: "guide".to_owned(),
        display_name: "Night Guide".to_owned(),
        description: String::new(),
        system_prompt: system_prompt.to_owned(),
        personality: String::new(),
        scenario: String::new(),
        first_message: String::new(),
        example_messages: String::new(),
        post_history_instructions: String::new(),
        lorebook: vec![PersonaLoreEntry {
            key: "harbor".to_owned(),
            content: "harbor lore".to_owned(),
            secondary_key: String::new(),
            comment: String::new(),
            always_active: false,
            selective: false,
            use_regex: false,
            insert_order: 0,
            enabled: true,
            case_sensitive: false,
            priority: 0,
        }],
        lore_settings: PersonaLoreSettings::default(),
        asset_kind: PersonaAssetKind::Card,
        source_kind: "fixture".to_owned(),
        asset_count: 0,
        ignored_features: BTreeMap::new(),
        tag_count: 0,
    }
}
