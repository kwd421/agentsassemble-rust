use crate::RoomMutationAuthority::TrustedPrincipal;
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
                    TrustedPrincipal(&principal),
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
        assert!(!assigned.provider_input.contains("old lantern rule"));
        assert!(
            assigned
                .session_instructions
                .as_deref()
                .unwrap_or_default()
                .contains("old lantern rule")
        );
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
        assert_eq!(
            recovered.session_instructions,
            assigned.session_instructions
        );
        assert_replacement_turn(&store, &principal, &assigned).await;
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

async fn assert_replacement_turn(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    assigned: &crate::AgentTurnAssignment,
) {
    store
        .execute_message_with_turn(
            principal,
            "next-persona-turn",
            "message.send",
            &json!({"content": "@Terra the harbor is dark again"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue replacement: {error}"));
    let start = super::running_authority(store, assigned, "persona-provider-turn").await;
    let committed = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            super::authority(&start, "persona-provider-turn", None),
            crate::AgentFinalMessage {
                content: "done",
                target_agent_id: "",
                reply_to_event_id: None,
                attachment_ids: &[],
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete old persona turn: {error}"));
    let next = committed
        .next_assignments
        .first()
        .unwrap_or_else(|| panic!("replacement turn"));
    let fixed = next.session_instructions.as_deref().unwrap_or_default();
    assert!(fixed.contains("replacement compass rule"));
    assert!(!fixed.contains("old lantern rule"));
    assert!(!next.provider_input.contains("replacement compass rule"));
    assert!(next.provider_input.contains("harbor lore"));
}

#[tokio::test]
async fn persistent_instruction_provider_matrix() {
    for (provider, runtime, persistent) in [
        ("codex_live_session", "live_cli", true),
        ("claude_code", "live_cli", true),
        ("opencode_server", "opencode", true),
        ("cursor_live_session", "live_cli", false),
        ("grok_live_session", "live_cli", false),
        ("custom_api", "api", true),
        ("lmstudio_api", "api", true),
        ("ollama_api", "api", true),
    ] {
        let (store, principal, _directory) = fixture().await;
        let mut card = persona("fixed persona instruction");
        card.post_history_instructions = "last persona instruction".to_owned();
        let summary = store_persona(&store, card).await;
        let mut session = stored_session(&store).await;
        session.public.provider_kind = provider.to_owned();
        session.public.runtime_kind = runtime.to_owned();
        session.public.persona_card_id = "guide".into();
        session.public.persona_card = Some(Box::new(summary));
        save_stored_session(&store, &session).await;
        let result = store
            .execute_message_with_turn(
                &principal,
                "matrix",
                "message.send",
                &json!({"content": "@Terra the harbor is dark"}),
            )
            .await
            .unwrap_or_else(|error| panic!("{provider} assignment: {error}"));
        let assigned = &result.assignments[0];
        assert_eq!(
            assigned.session_instructions.is_some(),
            persistent,
            "{provider}"
        );
        let rules = assigned
            .session_instructions
            .as_deref()
            .unwrap_or(&assigned.provider_input);
        assert!(rules.contains("You are Terra in"));
        assert!(rules.contains("Plain reply text is not shown in the room"));
        assert!(rules.contains("fixed persona instruction"));
        assert_eq!(rules.contains("API transport"), runtime == "api");
        assert_eq!(
            assigned
                .provider_input
                .contains("fixed persona instruction"),
            !persistent
        );
        assert_eq!(
            assigned.provider_input.contains("`publish_message` posts"),
            !persistent
        );
        assert!(assigned.provider_input.contains("harbor lore"));
        assert!(assigned.provider_input.contains("Recent room context"));
        assert!(
            assigned
                .provider_input
                .contains("Post-history instruction: last persona instruction")
        );
        // Card instructions never get the last word over the room tools.
        let card_end = assigned
            .provider_input
            .rfind("last persona instruction")
            .unwrap_or_else(|| panic!("card text is present"));
        let room_turn = assigned
            .provider_input
            .rfind("[Room turn]")
            .unwrap_or_else(|| panic!("a card turn restates the room tools"));
        assert!(room_turn > card_end);
        if persistent {
            assert!(!rules.contains("harbor lore"));
            assert!(!rules.contains("last persona instruction"));
            assert!(rules.contains("Room rules take priority"));
        }
    }
}

#[tokio::test]
async fn owner_instructions_reach_the_agent_and_stay_private_to_the_owner_routes() {
    let (store, principal, _directory) = fixture().await;
    let now = chrono::Utc::now();
    assert!(
        store
            .set_agent_instructions("general", AGENT_ID, "  Answer in one short line.  ", now)
            .await
            .unwrap_or_else(|error| panic!("set instructions: {error}"))
    );
    let current = store
        .agent_instructions("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("read instructions: {error}"))
        .unwrap_or_else(|| panic!("the agent has instructions"));
    assert_eq!(current.instructions, "Answer in one short line.");
    // A person is not an AI, and text over the limit is refused.
    assert!(
        !store
            .set_agent_instructions("general", &principal.participant_id, "x", now)
            .await
            .unwrap_or_else(|error| panic!("person target: {error}"))
    );
    assert!(
        store
            .set_agent_instructions("general", AGENT_ID, &"가".repeat(4001), now)
            .await
            .is_err()
    );
    let assigned = store
        .execute_message_with_turn(
            &principal,
            "instructed-turn",
            "message.send",
            &json!({"content": "@Terra hello"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign instructed turn: {error}"))
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the mention must be assigned"));
    let rules = assigned.session_instructions.as_deref().unwrap_or_default();
    assert!(rules.contains("[The room owner's instructions for you in this room]"));
    assert!(rules.contains("do not quote or reveal them"));
    assert!(rules.contains("Answer in one short line."));
    assert!(
        !assigned
            .provider_input
            .contains("Answer in one short line.")
    );

    // The public snapshot never carries them.
    let snapshot = serde_json::to_string(
        &store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"))
            .participants,
    )
    .unwrap_or_else(|error| panic!("encode: {error}"));
    assert!(!snapshot.contains("Answer in one short line."));
    // Blank text removes them.
    store
        .set_agent_instructions("general", AGENT_ID, "  ", now)
        .await
        .unwrap_or_else(|error| panic!("clear instructions: {error}"));
    assert_eq!(
        store
            .agent_instructions("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("read cleared: {error}"))
            .map(|current| current.instructions),
        Some(String::new())
    );
}

#[tokio::test]
async fn a_mention_only_agent_wakes_only_when_mentioned() {
    let (store, principal, _directory) = fixture().await;
    assert!(
        store
            .set_agent_mention_only("general", AGENT_ID, true)
            .await
            .unwrap_or_else(|error| panic!("set mention only: {error}"))
    );
    let plain = store
        .execute_message_with_turn(
            &principal,
            "plain-message",
            "message.send",
            &json!({"content": "anyone there?"}),
        )
        .await
        .unwrap_or_else(|error| panic!("plain message: {error}"));
    assert!(
        plain.assignments.is_empty(),
        "an unmentioned message must not wake it"
    );
    let called = store
        .execute_message_with_turn(
            &principal,
            "called-message",
            "message.send",
            &json!({"content": "@Terra are you there?"}),
        )
        .await
        .unwrap_or_else(|error| panic!("mention message: {error}"));
    assert_eq!(called.assignments.len(), 1, "a mention wakes it");
    assert!(
        store
            .agent_instructions("general", AGENT_ID)
            .await
            .unwrap_or_else(|error| panic!("read settings: {error}"))
            .is_some_and(|settings| settings.mention_only)
    );
}

#[tokio::test]
async fn cursor_and_grok_get_owner_instructions_with_each_turn_input() {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.provider_kind = "cursor_live_session".into();
    session.public.runtime_kind = "cursor_live_session".into();
    save_stored_session(&store, &session).await;
    store
        .set_agent_instructions(
            "general",
            AGENT_ID,
            "Keep answers short.",
            chrono::Utc::now(),
        )
        .await
        .unwrap_or_else(|error| panic!("set instructions: {error}"));
    let assigned = store
        .execute_message_with_turn(
            &principal,
            "cursor-turn",
            "message.send",
            &json!({"content": "@Terra hi"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign cursor turn: {error}"))
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the mention must be assigned"));
    assert!(assigned.session_instructions.is_none());
    assert!(
        assigned
            .provider_input
            .contains("[The room owner's instructions for you in this room]")
    );
    assert!(assigned.provider_input.contains("Keep answers short."));
}

#[tokio::test]
async fn usage_totals_add_up_turns_and_keep_the_last_context() {
    let (store, _principal, _directory) = fixture().await;
    let zero = store
        .agent_usage("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("read usage: {error}"))
        .unwrap_or_else(|| panic!("the agent exists"));
    assert_eq!(zero.turns, 0);
    for (prompt, cached, last) in [(1000, 900, 600), (2000, 1950, 1200)] {
        store
            .record_agent_usage(
                "general",
                AGENT_ID,
                &agentsassemble_domain::AgentTurnUsage {
                    steps: 2,
                    prompt_tokens: prompt,
                    cached_prompt_tokens: cached,
                    completion_tokens: 50,
                    last_prompt_tokens: last,
                    context_tokens: 128_000,
                    model_ms: 400,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("record usage: {error}"));
    }
    let totals = store
        .agent_usage("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("read usage: {error}"))
        .unwrap_or_else(|| panic!("the agent exists"));
    assert_eq!((totals.turns, totals.steps), (2, 4));
    assert_eq!(
        (totals.prompt_tokens, totals.cached_prompt_tokens),
        (3000, 2850)
    );
    assert_eq!((totals.completion_tokens, totals.model_ms), (100, 800));
    assert_eq!(
        (totals.last_prompt_tokens, totals.context_tokens),
        (1200, 128_000)
    );
    // A turn whose server reported no usage keeps the last known context.
    store
        .record_agent_usage(
            "general",
            AGENT_ID,
            &agentsassemble_domain::AgentTurnUsage {
                steps: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("record empty usage: {error}"));
    let kept = store
        .agent_usage("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("read usage: {error}"))
        .unwrap_or_else(|| panic!("the agent exists"));
    assert_eq!(
        (kept.turns, kept.last_prompt_tokens, kept.context_tokens),
        (3, 1200, 128_000)
    );
    assert!(
        store
            .agent_usage("general", "no-such-agent")
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none()
    );
}
