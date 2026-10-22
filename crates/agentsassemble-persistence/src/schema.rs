pub(crate) struct TableDefinition {
    pub name: &'static str,
    pub ddl: &'static str,
    pub infrastructure: bool,
}

pub(crate) const HOST_INITIALIZATION_DDL: &str = "CREATE TABLE IF NOT EXISTS runtime_host_initialization (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), nonce TEXT NOT NULL UNIQUE CHECK(length(nonce) = 36))";

pub(crate) const TABLES: &[TableDefinition] = &[
    TableDefinition {
        name: "delegated_action_requests",
        ddl: crate::delegated_confirmation::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "connector_message_delivery",
        ddl: crate::connector_delivery::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "connector_mention_overflow",
        ddl: crate::connector_delivery::OVERFLOW_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "ai_owner_bindings",
        ddl: crate::delegation_owner::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "central_member_removals",
        ddl: crate::central_member_removal::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "member_projection_outbox",
        ddl: crate::member_projection::DDL,
        infrastructure: true,
    },
    TableDefinition {
        name: "member_projection_sender",
        ddl: crate::member_projection::SENDER_DDL,
        infrastructure: true,
    },
    TableDefinition {
        name: "member_resume_keys",
        ddl: crate::member_resume::DDL,
        infrastructure: true,
    },
    TableDefinition {
        name: "central_identity_bindings",
        ddl: crate::central_identity_bindings::TABLE_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "member_admissions",
        ddl: crate::member_schema::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "central_owner_sessions",
        ddl: crate::schema_version::V74_OWNER_SESSIONS_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "host_owner_sessions",
        ddl: crate::host_owner_session::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "central_owner_grants",
        ddl: crate::server_owner_authority::GRANT_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_connector_uploads",
        ddl: crate::message_attachments::connector::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "provider_requests",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS provider_requests (",
            "room_id TEXT NOT NULL, request_id TEXT NOT NULL, session_id TEXT NOT NULL, ",
            "turn_generation INTEGER NOT NULL CHECK(turn_generation > 0), execution_id TEXT NOT NULL, ",
            "owner_id TEXT NOT NULL CHECK(length(owner_id)>0), request_json TEXT NOT NULL, ",
            "expires_at INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN ('open','resolving','resolved','denied','cancelled','expired','failed')), ",
            "open_event_id TEXT NOT NULL, resolution_json TEXT, resolution_fingerprint BLOB CHECK(resolution_fingerprint IS NULL OR length(resolution_fingerprint)=32), ",
            "connection_id TEXT, resolution_event_id TEXT, closed_event_id TEXT, ",
            "PRIMARY KEY(room_id,request_id), ",
            "FOREIGN KEY(room_id,session_id) REFERENCES agent_sessions(room_id,session_id) ON DELETE CASCADE) STRICT"
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "attendee_cleanup_controls",
        ddl: "CREATE TABLE IF NOT EXISTS attendee_cleanup_controls (room_id TEXT NOT NULL, session_id TEXT NOT NULL, cleanup_id TEXT NOT NULL CHECK(length(cleanup_id)=36), PRIMARY KEY(room_id,session_id), FOREIGN KEY(room_id,session_id) REFERENCES agent_sessions(room_id,session_id) ON DELETE CASCADE) STRICT",
        infrastructure: false,
    },
    TableDefinition {
        name: "attendee_connections",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS attendee_connections (",
            "session_fingerprint BLOB PRIMARY KEY CHECK(length(session_fingerprint)=32), ",
            "connection_id TEXT NOT NULL CHECK(length(connection_id)=36), ",
            "state TEXT NOT NULL CHECK(state IN ('connected','ready','disconnected')), ",
            "execution_os TEXT CHECK(execution_os IN ('macos','windows','linux','other')), ",
            "retained_interrupt INTEGER NOT NULL DEFAULT 0 CHECK(retained_interrupt IN (0,1)), ",
            "FOREIGN KEY(session_fingerprint) REFERENCES room_attendee_invites(session_fingerprint) ON DELETE CASCADE) STRICT"
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "room_attendee_invites",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS room_attendee_invites (",
            "invite_id TEXT PRIMARY KEY, room_id TEXT NOT NULL, room_uid TEXT NOT NULL, ",
            "creator_id TEXT NOT NULL, owner_participant_id TEXT NOT NULL, request_id TEXT NOT NULL, request_hash TEXT NOT NULL, ",
            "provider_kind TEXT NOT NULL, display_name TEXT NOT NULL, scope TEXT NOT NULL CHECK(scope IN ('read_write','read_only')), ",
            "parent_fingerprint BLOB CHECK(parent_fingerprint IS NULL OR length(parent_fingerprint)=32), ",
            "token_fingerprint BLOB NOT NULL UNIQUE CHECK(length(token_fingerprint)=32), ",
            "expires_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), ",
            "client_fingerprint BLOB CHECK(client_fingerprint IS NULL OR length(client_fingerprint)=32), ",
            "join_request_id TEXT, participant_id TEXT UNIQUE, event_id TEXT, ",
            "session_fingerprint BLOB UNIQUE CHECK(session_fingerprint IS NULL OR length(session_fingerprint)=32), session_expires_at INTEGER, ",
            "restore_parent_kind TEXT CHECK(restore_parent_kind IN ('human','operator')), ",
            "UNIQUE(room_id,creator_id,request_id), ",
            "CHECK((client_fingerprint IS NULL AND join_request_id IS NULL AND participant_id IS NULL AND event_id IS NULL AND session_fingerprint IS NULL AND session_expires_at IS NULL) OR ",
            "(client_fingerprint IS NOT NULL AND join_request_id IS NOT NULL AND participant_id IS NOT NULL AND event_id IS NOT NULL AND session_fingerprint IS NOT NULL AND session_expires_at IS NOT NULL)), ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE) STRICT"
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "connector_mentions",
        ddl: crate::connector_mentions::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_connector_invites",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS room_connector_invites (",
            "invite_id TEXT PRIMARY KEY, room_id TEXT NOT NULL, room_uid TEXT NOT NULL, ",
            "creator_id TEXT NOT NULL, request_id TEXT NOT NULL, scope TEXT NOT NULL CHECK(scope IN ('read_write','read_only')), ",
            "token_fingerprint BLOB NOT NULL UNIQUE CHECK(length(token_fingerprint) = 32), ",
            "expires_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)), ",
            "client_fingerprint BLOB CHECK(client_fingerprint IS NULL OR length(client_fingerprint) = 32), ",
            "join_request_id TEXT, join_payload_hash TEXT, participant_id TEXT UNIQUE, event_id TEXT, ",
            "session_fingerprint BLOB UNIQUE CHECK(session_fingerprint IS NULL OR length(session_fingerprint) = 32), ",
            "session_expires_at INTEGER, UNIQUE(room_id, request_id), ",
            "CHECK((client_fingerprint IS NULL AND join_request_id IS NULL AND join_payload_hash IS NULL AND participant_id IS NULL AND event_id IS NULL AND session_fingerprint IS NULL AND session_expires_at IS NULL) OR ",
            "(client_fingerprint IS NOT NULL AND join_request_id IS NOT NULL AND join_payload_hash IS NOT NULL AND participant_id IS NOT NULL AND event_id IS NOT NULL AND session_fingerprint IS NOT NULL AND session_expires_at IS NOT NULL)), ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE) STRICT"
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "runtime_metadata",
        ddl: "CREATE TABLE IF NOT EXISTS runtime_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        infrastructure: true,
    },
    TableDefinition {
        name: "runtime_host_initialization",
        ddl: HOST_INITIALIZATION_DDL,
        infrastructure: true,
    },
    TableDefinition {
        name: "local_bootstrap_authority",
        ddl: "CREATE TABLE IF NOT EXISTS local_bootstrap_authority (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), authority_lineage_id TEXT NOT NULL UNIQUE, state TEXT NOT NULL CHECK(state IN ('empty', 'initializing', 'complete')), request_id TEXT NOT NULL DEFAULT '', schema_revision INTEGER NOT NULL CHECK(schema_revision > 0), initialization_digest TEXT NOT NULL DEFAULT '', user_id TEXT NOT NULL DEFAULT '', participant_id TEXT NOT NULL DEFAULT '', result_json TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, completed_at TEXT)",
        infrastructure: true,
    },
    TableDefinition {
        name: "runtime_host_identity",
        ddl: "CREATE TABLE IF NOT EXISTS runtime_host_identity (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), server_id TEXT NOT NULL UNIQUE, public_key BLOB NOT NULL CHECK(typeof(public_key) = 'blob' AND length(public_key) = 32), session_hmac_key_fingerprint BLOB NOT NULL CHECK(typeof(session_hmac_key_fingerprint) = 'blob' AND length(session_hmac_key_fingerprint) = 32))",
        infrastructure: true,
    },
    TableDefinition {
        name: "rooms",
        ddl: "CREATE TABLE IF NOT EXISTS rooms (room_id TEXT PRIMARY KEY, room_json TEXT NOT NULL, settings_json TEXT NOT NULL)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_create_results",
        ddl: "CREATE TABLE IF NOT EXISTS room_create_results (principal_id TEXT NOT NULL, request_id TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(principal_id, request_id))",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_channel_retirements",
        ddl: "CREATE TABLE IF NOT EXISTS room_channel_retirements (room_id TEXT NOT NULL, channel_id TEXT NOT NULL, PRIMARY KEY(room_id, channel_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE) STRICT",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_delete_results",
        ddl: "CREATE TABLE IF NOT EXISTS room_delete_results (room_id TEXT NOT NULL, room_uid TEXT NOT NULL UNIQUE, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'complete')), PRIMARY KEY(room_id, principal_id, request_id))",
        infrastructure: false,
    },
    TableDefinition {
        name: "operator_pairings",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS operator_pairings (",
            "pairing_id TEXT PRIMARY KEY, ",
            "token_fingerprint BLOB NOT NULL UNIQUE CHECK(length(token_fingerprint) = 32), ",
            "room_id TEXT NOT NULL, room_uid TEXT NOT NULL, server_id TEXT NOT NULL, ",
            "authority_lineage_id TEXT NOT NULL, user_id TEXT NOT NULL, participant_id TEXT NOT NULL, ",
            "target_origin TEXT NOT NULL, expires_at INTEGER NOT NULL, ",
            "central_owner INTEGER NOT NULL DEFAULT 0 CHECK(central_owner IN (0, 1)), ",
            "owner_session_fingerprint BLOB REFERENCES central_owner_sessions(fingerprint) ON DELETE CASCADE, ",
            "host_owner_session_fingerprint BLOB REFERENCES host_owner_sessions(fingerprint) ON DELETE CASCADE, ",
            "device_name TEXT NOT NULL DEFAULT '', browser TEXT NOT NULL DEFAULT '', os TEXT NOT NULL DEFAULT '', last_connected_at INTEGER, ",
            "revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0, 1)), ",
            "device_fingerprint BLOB CHECK(device_fingerprint IS NULL OR length(device_fingerprint) = 32), ",
            "session_fingerprint BLOB UNIQUE CHECK(session_fingerprint IS NULL OR length(session_fingerprint) = 32), ",
            "session_expires_at INTEGER, ",
            "CHECK((device_fingerprint IS NULL AND session_fingerprint IS NULL AND session_expires_at IS NULL) OR ",
            "(device_fingerprint IS NOT NULL AND session_fingerprint IS NOT NULL AND session_expires_at IS NOT NULL)), ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE) STRICT",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "participants",
        ddl: "CREATE TABLE IF NOT EXISTS participants (room_id TEXT NOT NULL, participant_id TEXT NOT NULL, participant_json TEXT NOT NULL, PRIMARY KEY(room_id, participant_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "user_profiles",
        ddl: "CREATE TABLE IF NOT EXISTS user_profiles (user_id TEXT PRIMARY KEY, participant_id TEXT NOT NULL UNIQUE, profile_json TEXT NOT NULL, UNIQUE(user_id, participant_id))",
        infrastructure: false,
    },
    TableDefinition {
        name: "persona_assets",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS persona_assets (",
            "persona_id TEXT PRIMARY KEY CHECK(typeof(persona_id) = 'text' ",
            "AND length(CAST(persona_id AS BLOB)) BETWEEN 1 AND 80 ",
            "AND instr(persona_id, char(0)) = 0), ",
            "card_json TEXT NOT NULL CHECK(typeof(card_json) = 'text' ",
            "AND length(CAST(card_json AS BLOB)) BETWEEN 2 AND 10485760), ",
            "thumbnail_png BLOB CHECK(thumbnail_png IS NULL OR ",
            "(typeof(thumbnail_png) = 'blob' ",
            "AND length(thumbnail_png) BETWEEN 1 AND 10485760)))",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "profile_avatar_assets",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS profile_avatar_assets (",
            "attachment_id TEXT PRIMARY KEY, ",
            "owner_user_id TEXT NOT NULL, ",
            "filename TEXT NOT NULL, ",
            "content_type TEXT NOT NULL CHECK(content_type = 'image/png'), ",
            "content BLOB NOT NULL, ",
            "size INTEGER NOT NULL CHECK(size > 0 AND size <= 10485760), ",
            "created_at TEXT NOT NULL, ",
            "state TEXT NOT NULL CHECK(state IN ('pending', 'current')), ",
            "expires_at INTEGER, ",
            "CHECK((state = 'pending' AND expires_at IS NOT NULL) OR ",
            "(state = 'current' AND expires_at IS NULL)), ",
            "FOREIGN KEY(owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "prejoin_avatar_assets",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS prejoin_avatar_assets (",
            "attachment_id TEXT PRIMARY KEY, ",
            "room_id TEXT NOT NULL, ",
            "custody_fingerprint BLOB NOT NULL CHECK(typeof(custody_fingerprint) = 'blob' AND length(custody_fingerprint) = 32), ",
            "invite_fingerprint BLOB NOT NULL CHECK(typeof(invite_fingerprint) = 'blob' AND length(invite_fingerprint) = 32), ",
            "filename TEXT NOT NULL, ",
            "content_type TEXT NOT NULL CHECK(content_type = 'image/png'), ",
            "content BLOB NOT NULL, ",
            "size INTEGER NOT NULL CHECK(size > 0 AND size <= 10485760), ",
            "created_at TEXT NOT NULL, ",
            "expires_at INTEGER NOT NULL, ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "room_user_preferences",
        ddl: "CREATE TABLE IF NOT EXISTS room_user_preferences (user_id TEXT NOT NULL, room_id TEXT NOT NULL, preferences_json TEXT NOT NULL, PRIMARY KEY(user_id, room_id), FOREIGN KEY(user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE, FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_invites",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS room_invites (",
            "invite_id TEXT PRIMARY KEY CHECK(",
            "typeof(invite_id) = 'text' AND length(invite_id) = 16 ",
            "AND invite_id = lower(invite_id) ",
            "AND invite_id NOT GLOB '*[^0-9a-f]*' ",
            "AND invite_id = lower(substr(hex(signed_token_fingerprint), 1, 16))), ",
            "signed_token_fingerprint BLOB NOT NULL UNIQUE ",
            "CHECK(typeof(signed_token_fingerprint) = 'blob' ",
            "AND length(signed_token_fingerprint) = 32), ",
            "join_code_fingerprint BLOB NOT NULL UNIQUE ",
            "CHECK(typeof(join_code_fingerprint) = 'blob' ",
            "AND length(join_code_fingerprint) = 32), ",
            "room_id TEXT NOT NULL, ",
            "base_participant_id TEXT NOT NULL CHECK(length(base_participant_id) > 0), ",
            "display_name TEXT NOT NULL CHECK(length(display_name) > 0), ",
            "invite_scope TEXT NOT NULL CHECK(invite_scope IN ('read_write', 'read_only')), ",
            "max_uses INTEGER NOT NULL CHECK(max_uses >= 0), ",
            "key_kind TEXT GENERATED ALWAYS AS (CASE ",
            "WHEN max_uses = 1 THEN 'one_use' ELSE 'reusable' END) STORED, ",
            "use_count INTEGER NOT NULL DEFAULT 0 CHECK(use_count >= 0), ",
            "expires_at INTEGER NOT NULL, ",
            "revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0, 1)), ",
            "created_by_user_id TEXT NOT NULL, ",
            "created_at INTEGER NOT NULL, ",
            "CHECK(expires_at > created_at), ",
            "UNIQUE(invite_id, room_id, invite_scope, key_kind), ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(created_by_user_id) REFERENCES user_profiles(user_id))",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "google_accounts",
        ddl: "CREATE TABLE IF NOT EXISTS google_accounts (subject_fingerprint BLOB PRIMARY KEY CHECK(typeof(subject_fingerprint) = 'blob' AND length(subject_fingerprint) = 32), user_id TEXT NOT NULL UNIQUE, FOREIGN KEY(user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "saved_friends",
        ddl: "CREATE TABLE IF NOT EXISTS saved_friends (friend_id TEXT PRIMARY KEY, friend_json TEXT)",
        infrastructure: false,
    },
    TableDefinition {
        name: "human_device_credentials",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS human_device_credentials (",
            "credential_fingerprint BLOB PRIMARY KEY ",
            "CHECK(typeof(credential_fingerprint) = 'blob' ",
            "AND length(credential_fingerprint) = 32), ",
            "user_id TEXT NOT NULL, ",
            "created_at INTEGER NOT NULL, ",
            "UNIQUE(credential_fingerprint, user_id), ",
            "FOREIGN KEY(user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "guest_recovery_codes",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS guest_recovery_codes (",
            "user_id TEXT PRIMARY KEY REFERENCES user_profiles(user_id) ON DELETE CASCADE, ",
            "fingerprint BLOB NOT NULL UNIQUE CHECK(typeof(fingerprint) = 'blob' AND length(fingerprint) = 32), ",
            "previous_fingerprint BLOB UNIQUE CHECK(previous_fingerprint IS NULL OR (typeof(previous_fingerprint) = 'blob' AND length(previous_fingerprint) = 32)), ",
            "session_key BLOB REFERENCES human_room_sessions(admission_key) ON DELETE SET NULL)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "human_room_sessions",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS human_room_sessions (",
            "admission_key BLOB PRIMARY KEY ",
            "CHECK(typeof(admission_key) = 'blob' AND length(admission_key) = 32), ",
            "key_kind TEXT NOT NULL CHECK(key_kind IN ('one_use', 'reusable', 'recovery')), ",
            "member_admission_id TEXT REFERENCES member_admissions(admission_id), ",
            "member_challenge BLOB CHECK(member_challenge IS NULL OR length(member_challenge) = 32), ",
            "member_challenge_expires_at INTEGER NOT NULL DEFAULT 0, ",
            "member_last_used_at INTEGER NOT NULL DEFAULT 0, ",
            "secure_client_key_fingerprint BLOB CHECK(secure_client_key_fingerprint IS NULL OR length(secure_client_key_fingerprint)=32), ",
            "secure_channel_id TEXT CHECK(secure_channel_id IS NULL OR length(secure_channel_id)=43), ",
            "first_request_id TEXT CHECK(length(first_request_id) = 36), ",
            "invite_id TEXT, ",
            "payload_hash BLOB ",
            "CHECK(payload_hash IS NULL OR (typeof(payload_hash) = 'blob' AND length(payload_hash) = 32)), ",
            "session_fingerprint BLOB NOT NULL UNIQUE ",
            "CHECK(typeof(session_fingerprint) = 'blob' AND length(session_fingerprint) = 32), ",
            "room_id TEXT NOT NULL, ",
            "user_id TEXT NOT NULL, ",
            "participant_id TEXT NOT NULL, ",
            "client_kind TEXT NOT NULL CHECK(client_kind = 'browser'), ",
            "invite_scope TEXT NOT NULL CHECK(invite_scope IN ('read_write', 'read_only')), ",
            "browser_credential_fingerprint BLOB NOT NULL ",
            "CHECK(typeof(browser_credential_fingerprint) = 'blob' ",
            "AND length(browser_credential_fingerprint) = 32), ",
            "reusable_identity_fingerprint BLOB ",
            "CHECK(reusable_identity_fingerprint IS NULL OR ",
            "(typeof(reusable_identity_fingerprint) = 'blob' ",
            "AND length(reusable_identity_fingerprint) = 32)), ",
            "result_json TEXT NOT NULL, ",
            "admitted_at INTEGER NOT NULL, ",
            "expires_at INTEGER NOT NULL, ",
            "state TEXT NOT NULL CHECK(state IN ('active', 'ended')), ",
            "CHECK(expires_at > admitted_at), ",
            "CHECK((key_kind = 'recovery' AND invite_id IS NULL AND first_request_id IS NULL AND payload_hash IS NULL) OR ",
            "(key_kind IN ('one_use', 'reusable') AND invite_id IS NOT NULL AND first_request_id IS NOT NULL AND payload_hash IS NOT NULL)), ",
            "CHECK(",
            "(member_admission_id IS NOT NULL AND key_kind != 'recovery' AND reusable_identity_fingerprint IS NULL) OR ",
            "(member_admission_id IS NULL AND ((key_kind = 'one_use' AND reusable_identity_fingerprint IS NULL) OR ",
            "(key_kind IN ('reusable', 'recovery') AND reusable_identity_fingerprint IS NOT NULL ",
            "AND length(reusable_identity_fingerprint) = 32 ",
            "AND reusable_identity_fingerprint = browser_credential_fingerprint)))), ",
            "FOREIGN KEY(invite_id, room_id, invite_scope, key_kind) ",
            "REFERENCES room_invites(invite_id, room_id, invite_scope, key_kind) ",
            "ON DELETE CASCADE, ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(user_id, participant_id) ",
            "REFERENCES user_profiles(user_id, participant_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(reusable_identity_fingerprint, user_id) ",
            "REFERENCES human_device_credentials(credential_fingerprint, user_id), ",
            "FOREIGN KEY(room_id, participant_id) ",
            "REFERENCES participants(room_id, participant_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "room_appearance_assets",
        ddl: "CREATE TABLE IF NOT EXISTS room_appearance_assets (asset_id TEXT NOT NULL PRIMARY KEY CHECK(typeof(asset_id) = 'text' AND length(CAST(asset_id AS BLOB)) = 35 AND asset_id GLOB 'ra_[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'), room_id TEXT NOT NULL, pending_owner_user_id TEXT, filename TEXT NOT NULL, content_type TEXT NOT NULL CHECK(content_type = 'image/png'), content BLOB NOT NULL, size INTEGER NOT NULL CHECK(size > 0 AND size <= 10485760), created_at TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'bound')), expires_at INTEGER, CHECK((state = 'pending' AND pending_owner_user_id IS NOT NULL AND expires_at IS NOT NULL) OR (state = 'bound' AND pending_owner_user_id IS NULL AND expires_at IS NULL)), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE, FOREIGN KEY(pending_owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "agent_sessions",
        ddl: "CREATE TABLE IF NOT EXISTS agent_sessions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, session_json TEXT NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_runtime_cleanup",
        ddl: "CREATE TABLE IF NOT EXISTS room_runtime_cleanup (room_id TEXT NOT NULL, session_id TEXT NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE RESTRICT)",
        infrastructure: false,
    },
    TableDefinition {
        name: "agent_avatar_assets",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS agent_avatar_assets (",
            "asset_id TEXT NOT NULL PRIMARY KEY CHECK(length(asset_id) = 35 AND substr(asset_id, 1, 3) = 'aa_' AND substr(asset_id, 4) NOT GLOB '*[^0-9a-f]*'), ",
            "room_id TEXT NOT NULL, session_id TEXT NOT NULL, filename TEXT NOT NULL, ",
            "content_type TEXT NOT NULL CHECK(content_type = 'image/png'), content BLOB NOT NULL, ",
            "size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 10485760 AND length(content) = size), ",
            "created_at TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'current')), expires_at INTEGER, ",
            "CHECK((state = 'pending' AND expires_at IS NOT NULL) OR (state = 'current' AND expires_at IS NULL)), ",
            "UNIQUE(room_id, session_id, state), ",
            "FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "provider_turn_executions",
        ddl: "CREATE TABLE IF NOT EXISTS provider_turn_executions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_generation INTEGER NOT NULL CHECK(turn_generation > 0), execution_id TEXT NOT NULL, participant_id TEXT NOT NULL, turn_id TEXT NOT NULL, assignment_json TEXT NOT NULL, phase TEXT NOT NULL CHECK(phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required', 'completed', 'declined', 'failed', 'interrupted')), runtime_handle_id TEXT NOT NULL CHECK(length(runtime_handle_id) > 0), runtime_owner_id TEXT NOT NULL CHECK(length(runtime_owner_id) > 0), runtime_lease_token TEXT NOT NULL CHECK(length(runtime_lease_token) > 0), start_dispatch_nonce TEXT NOT NULL DEFAULT '', provider_turn_id TEXT NOT NULL DEFAULT '', requeue_finalized INTEGER NOT NULL DEFAULT 0 CHECK(requeue_finalized IN (0, 1)), released_input_ids TEXT CHECK(released_input_ids IS NULL OR (json_valid(released_input_ids) AND json_type(released_input_ids) = 'array')), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(room_id, session_id, turn_generation), UNIQUE(room_id, execution_id), UNIQUE(room_id, session_id, turn_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE, FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "provider_turn_effects",
        ddl: "CREATE TABLE IF NOT EXISTS provider_turn_effects (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_generation INTEGER NOT NULL CHECK(turn_generation > 0), effect_id TEXT NOT NULL, effect_kind TEXT NOT NULL CHECK(effect_kind = 'interrupt'), interrupt_cause TEXT NOT NULL CHECK(interrupt_cause IN ('participant_muted', 'agent_interrupt')), phase TEXT NOT NULL CHECK(phase IN ('prepared', 'claimed', 'dispatching', 'issued_waiting_quiescence', 'interrupt_ambiguous', 'recovery_required', 'finalized')), claim_owner TEXT NOT NULL DEFAULT '', claim_expires_at INTEGER, dispatch_nonce TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(room_id, effect_id), UNIQUE(room_id, session_id, turn_generation, effect_kind), FOREIGN KEY(room_id, session_id, turn_generation) REFERENCES provider_turn_executions(room_id, session_id, turn_generation) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "lifecycle_command_reservations",
        ddl: "CREATE TABLE IF NOT EXISTS lifecycle_command_reservations (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, principal_json TEXT NOT NULL, payload_json TEXT NOT NULL, supervisor_generation TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'rejected')), phase TEXT NOT NULL DEFAULT 'lifecycle_prepared', prepared_result_json TEXT NOT NULL DEFAULT '{}', failure_code TEXT NOT NULL DEFAULT '', failure_message TEXT NOT NULL DEFAULT '', PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "ordered_input_routes",
        ddl: crate::ordered_input_route::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_events",
        ddl: "CREATE TABLE IF NOT EXISTS room_events (room_id TEXT NOT NULL, seq INTEGER NOT NULL CHECK(seq > 0), event_json TEXT NOT NULL, PRIMARY KEY(room_id, seq), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_message_attachments",
        ddl: crate::message_attachments::DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_message_attachment_previews",
        ddl: crate::message_attachment_previews::MESSAGE_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_connector_upload_previews",
        ddl: crate::message_attachment_previews::CONNECTOR_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_message_attachment_chunks",
        ddl: crate::room_file_content::MESSAGE_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_connector_upload_chunks",
        ddl: crate::room_file_content::CONNECTOR_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_message_pins",
        ddl: "CREATE TABLE IF NOT EXISTS room_message_pins (room_id TEXT NOT NULL, event_id TEXT NOT NULL CHECK(typeof(event_id) = 'text' AND length(CAST(event_id AS BLOB)) BETWEEN 1 AND 128 AND instr(event_id, char(0)) = 0), event_seq INTEGER NOT NULL CHECK(event_seq > 0), pinned_at INTEGER NOT NULL CHECK(pinned_at > 0), PRIMARY KEY(room_id, event_id), UNIQUE(room_id, event_seq), FOREIGN KEY(room_id, event_seq) REFERENCES room_events(room_id, seq) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_message_search_records",
        ddl: crate::schema_message_search::RECORDS_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_message_search_phrase",
        ddl: crate::schema_message_search::PHRASE_DDL,
        infrastructure: false,
    },
    TableDefinition {
        name: "room_turn_tool_results",
        ddl: "CREATE TABLE IF NOT EXISTS room_turn_tool_results (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL, result_id TEXT NOT NULL, event_seq INTEGER NOT NULL CHECK(event_seq > 0), PRIMARY KEY(room_id, result_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_write_budgets",
        ddl: "CREATE TABLE IF NOT EXISTS room_write_budgets (room_id TEXT NOT NULL, window_started_at INTEGER NOT NULL, command_count INTEGER NOT NULL CHECK(command_count >= 0), payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0), PRIMARY KEY(room_id, window_started_at), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_event_publication_cursors",
        ddl: "CREATE TABLE IF NOT EXISTS room_event_publication_cursors (room_id TEXT PRIMARY KEY, published_seq INTEGER NOT NULL CHECK(published_seq >= 0), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "command_results",
        ddl: "CREATE TABLE IF NOT EXISTS command_results (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
];

pub(crate) const INDEXES: &[&str] = &[
    crate::agent_avatar_assets::EXPIRY_INDEX_DDL,
    crate::central_identity_bindings::USER_INDEX_DDL,
    crate::member_resume::LOOKUP_INDEX_DDL,
    "CREATE UNIQUE INDEX IF NOT EXISTS provider_requests_pending_session_idx ON provider_requests(room_id,session_id) WHERE state IN ('open','resolving')",
    "CREATE INDEX IF NOT EXISTS room_channel_messages_idx ON room_events(room_id, json_extract(event_json, '$.channel_id'), seq) WHERE json_extract(event_json, '$.type') = 'channel_message_final'",
    "CREATE UNIQUE INDEX IF NOT EXISTS room_delete_pending_idx ON room_delete_results(room_id) WHERE state = 'pending'",
    "CREATE INDEX IF NOT EXISTS agent_avatar_assets_expiry_idx ON agent_avatar_assets(expires_at) WHERE state = 'pending'",
    "CREATE UNIQUE INDEX IF NOT EXISTS profile_avatar_assets_owner_state_idx ON profile_avatar_assets(owner_user_id, state)",
    "CREATE INDEX IF NOT EXISTS profile_avatar_assets_state_expiry_idx ON profile_avatar_assets(state, expires_at)",
    "CREATE UNIQUE INDEX IF NOT EXISTS prejoin_avatar_assets_custody_idx ON prejoin_avatar_assets(custody_fingerprint)",
    "CREATE INDEX IF NOT EXISTS prejoin_avatar_assets_expiry_idx ON prejoin_avatar_assets(expires_at)",
    "CREATE INDEX IF NOT EXISTS room_invites_room_state_idx ON room_invites(room_id, revoked, expires_at)",
    "CREATE UNIQUE INDEX IF NOT EXISTS human_room_sessions_active_participant_idx ON human_room_sessions(room_id, participant_id) WHERE state = 'active' AND member_admission_id IS NULL",
    "CREATE UNIQUE INDEX IF NOT EXISTS human_room_sessions_member_device_idx ON human_room_sessions(member_admission_id, browser_credential_fingerprint) WHERE state = 'active' AND member_admission_id IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_member_history_idx ON human_room_sessions(member_admission_id, state, admitted_at)",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_live_idx ON human_room_sessions(state, expires_at)",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_room_live_idx ON human_room_sessions(room_id, state, expires_at)",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_invite_state_idx ON human_room_sessions(invite_id, key_kind, state)",
    "CREATE INDEX IF NOT EXISTS room_appearance_assets_pending_idx ON room_appearance_assets(pending_owner_user_id, expires_at) WHERE state = 'pending'",
    "CREATE INDEX IF NOT EXISTS room_appearance_assets_room_idx ON room_appearance_assets(room_id, state)",
    "CREATE INDEX IF NOT EXISTS room_message_attachments_pending_expiry_idx ON room_message_attachments(expires_at) WHERE state = 'pending'",
    "CREATE INDEX IF NOT EXISTS room_message_attachments_event_idx ON room_message_attachments(room_id, event_seq) WHERE state = 'bound'",
    "CREATE INDEX IF NOT EXISTS room_write_budgets_window_idx ON room_write_budgets(window_started_at)",
    "CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_executions_blocking_session_idx ON provider_turn_executions(room_id, session_id) WHERE phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required')",
    "CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_executions_blocking_runtime_idx ON provider_turn_executions(runtime_handle_id) WHERE phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required')",
    "CREATE INDEX IF NOT EXISTS lifecycle_command_reservations_pending_session_idx ON lifecycle_command_reservations(room_id, session_id) WHERE status = 'pending'",
];

pub(crate) fn statements() -> impl Iterator<Item = &'static str> {
    tables()
        .map(|table| table.ddl)
        .chain(INDEXES.iter().copied())
        .chain(crate::schema_message_search::AUXILIARY_DDL.iter().copied())
}

pub(crate) fn product_tables() -> impl Iterator<Item = &'static TableDefinition> {
    tables().filter(|table| !table.infrastructure)
}

pub(crate) fn tables() -> impl Iterator<Item = &'static TableDefinition> {
    TABLES
        .iter()
        .chain(crate::schema_votes::TABLES.iter())
        .chain(crate::agent_memory::TABLES.iter())
        .chain(crate::agent_avatar_assets::TABLES.iter())
        .chain(crate::connector_connections::TABLES.iter())
        .chain(crate::agent_instructions::TABLES.iter())
        .chain(crate::agent_usage::TABLES.iter())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "schema_human_session_tests.rs"]
mod human_session_tests;

#[cfg(test)]
#[path = "schema_asset_tests.rs"]
mod asset_tests;

#[cfg(test)]
#[path = "schema_message_pin_tests.rs"]
mod message_pin_tests;

#[cfg(test)]
#[path = "schema_message_attachment_tests.rs"]
mod message_attachment_tests;
