// Test scenarios may run long; production functions stay under the line limit.
#![cfg_attr(test, allow(clippy::too_many_lines))]
mod delegated_agent_control;
mod delegated_direct;
mod delegated_event_attribution;
pub use delegated_agent_control::{AgentControlAuthority, DelegatedAgentControl};
mod delegated_pin;
pub use delegated_pin::DelegateRoomAuthority;
mod account_data_reset;
mod agent_handoff_trust;
mod agent_instructions;
mod agent_usage;
pub use agent_instructions::{AgentInstructions, MAX_AGENT_INSTRUCTIONS_CHARS};
mod agent_memory;
mod archive_schema;
pub mod backup_codec;
pub mod backup_database;
mod backup_json;
mod backup_policy;
mod delegation_owner;
pub mod restore_activation;
pub mod restore_job;
mod restore_preparation;
mod restore_ready;
pub use account_data_reset::AccountDataResetOutcome;
mod account_deleted_host;
mod attendee;
#[cfg(test)]
mod central_removal_owner_tests;
mod host_owner_session;
#[cfg(test)]
mod host_owner_session_tests;
mod owner_devices;
mod server_owner_authority;
pub use host_owner_session::{
    OWNER_SESSION_PREFIX, OwnerAdmission, OwnerAdmissionBinding, OwnerDeviceDescription,
    OwnerSessionAuthorization, OwnerSessionRedemption,
};
pub use owner_devices::{OwnerDeviceSession, OwnerDevicesRevocation};
pub use server_owner_authority::{ServerOwnerAuthority, ServerOwnerLifecycleAuthorization};
mod local_attendee_receipt;
pub use attendee::admission::{AttendeeAdmission, AttendeeAdmissionRequest};
pub use attendee::cleanup::{
    AttendeeCleanupAuthorization, AttendeeCleanupDelivery, AttendeeCleanupReport,
};
pub use attendee::connection::{AttendeeConnectionAuthorization, AttendeeConnectionClaim};
pub use attendee::interrupt::{
    AttendeeInterruptDelivery, AttendeeInterruptReport, AttendeeInterruptedRuntime,
};
pub use attendee::invites::{AttendeeInvite, CompanionInviteRequest};
pub use attendee::random::{AttendeeRandomMutation, AttendeeRandomRequest};
pub use attendee::ready::AttendeeRuntimeReady;
pub use attendee::session::AttendeeSessionAuthorization;
pub use attendee::tool_read::{AttendeeToolRead, AttendeeToolReadRequest, AttendeeToolReadResult};
mod connector_admission;
mod connector_connections;
#[cfg(test)]
mod connector_file_parity_tests;
mod connector_mentions;
mod connector_session;
#[cfg(test)]
mod connector_tests;
pub use connector_admission::{ConnectorAdmission, ConnectorInvite};
pub use connector_connections::ResumedConnector;
pub use connector_session::ConnectorSessionAuthorization;
mod account_guest_retirement;
#[cfg(test)]
mod google_account_tests;
mod google_accounts;
pub use google_accounts::{GoogleAccount, GoogleAccountLink};
mod account_identity;
pub use account_identity::{AccountAuthority, AccountIdentity, AccountUser};
mod agent_avatar_assets;
mod participant_rows;
pub use agent_avatar_assets::{AgentAvatarAsset, AgentAvatarMetadata};
mod agent_configuration;
mod agent_create_start;
mod agent_creation_records;
mod agent_interrupt;
mod agent_launch_events;
mod agent_lifecycle;
mod agent_lifecycle_authority;
mod agent_lifecycle_effect_authority;
mod agent_lifecycle_events;
mod agent_lifecycle_reservations;
mod agent_pause;
mod agent_profile;
mod agent_readd;
mod agent_reconciliation;
mod agent_reconciliation_live;
mod agent_reconciliation_recovery;
mod agent_reconciliation_scan;
mod agent_session_rows;
mod agent_sessions;
mod agent_start_failure;
mod agent_stop_lifecycle;
mod asset_storage;
mod authority;
pub use authority::RoomMutationAuthority;
mod room_session_authority;
pub use room_session_authority::RoomSessionAuthorization;
mod bootstrap;
mod central_endpoint;
pub use central_endpoint::CentralEndpointEvent;
mod named_tunnel;
pub use named_tunnel::{StoredNamedTunnel, is_valid_named_tunnel_hostname};
mod channel_messages;
mod command_admission;
mod runtime_restart;
mod runtime_restart_recovery;
pub use runtime_restart::{RuntimeRestartPhase, RuntimeRestartRecord, RuntimeRestartTarget};
mod database_target;
mod filesystem_authority;
mod friends;
#[cfg(test)]
mod friends_tests;
mod host_identity;
mod host_key_file;
mod operator_pairing;
mod room_channels;
mod side_chat;
pub use operator_pairing::{
    CentralOwnerSessionRequest, OperatorPairing, OperatorPairingRedemption,
    OperatorSessionAuthorization,
};
pub use side_chat::SideChatCommit;
mod guest_identity_recovery;
mod human_admission;
mod human_admission_identity;
mod human_admission_store;
pub use human_admission_store::{
    DEBUG_ROOM_SESSION_TTL_ENV, DEBUG_ROOM_SESSION_TTL_MAX_SECS, DEBUG_ROOM_SESSION_TTL_MIN_SECS,
    debug_room_session_ttl_override, default_room_session_ttl, resolve_room_session_ttl,
};
mod session_bearer;
pub use session_bearer::{
    ATTENDEE_INVITE_PREFIX, ATTENDEE_SESSION_PREFIX, CONNECTOR_INVITE_PREFIX,
    CONNECTOR_SESSION_PREFIX, GUEST_RECOVERY_CODE_PREFIX, HUMAN_SESSION_BEARER_BYTES,
    HUMAN_SESSION_BEARER_CHARS, HUMAN_SESSION_BEARER_PREFIX, OPERATOR_SESSION_BEARER_CHARS,
    OPERATOR_SESSION_BEARER_PREFIX,
};
mod conversation_status;
mod human_invite_preflight;
mod human_invites;
mod human_prejoin_attachments;
mod human_session_authority;
mod message_attachment_previews;
mod message_attachments;
mod provider_file;
pub use provider_file::AttendeeFileAuthority;
pub use provider_file::ConnectorFileAuthority;
pub use provider_file::ProviderFileAuthority;
mod message_upload;
pub use message_upload::{MESSAGE_UPLOAD_PIECE_BYTES, MessageUploadAuthority, MessageUploadStatus};
mod message_mutations;
mod message_pins;
mod message_replies;
mod message_search;
mod message_search_index;
mod private_fs;
mod profile_attachments;
mod profile_store;
mod raster_assets;
mod raster_validation_failure;
pub use raster_validation_failure::RasterValidationFailure;
mod raster_preview;
#[cfg(test)]
mod raster_preview_tests;
mod room_appearance_assets;
mod room_deletion;
mod room_directory;
mod room_lifecycle;
pub use room_deletion::{RoomDeletionMutation, RoomDeletionPage};
pub use room_lifecycle::RoomLifecycleMutation;
mod provider_request_authority;
mod provider_request_completion;
mod provider_request_lifecycle;
pub use provider_request_completion::ProviderRequestDeliveryOutcome;
mod provider_request_resolution;
mod provider_requests;
pub use provider_request_resolution::{ProviderRequestDelivery, ProviderRequestResolutionCommit};
mod room_event_publication;
mod room_event_sequence;
mod room_history;
mod room_preferences;
mod room_random;
pub use provider_requests::{OpenProviderRequest, ProviderRequestCommit};
mod central_identity_bindings;
mod central_member_removal;
pub use central_member_removal::{MemberRemovalKey, MemberRemovalPrincipal};
mod central_removal_authority;
pub use central_removal_authority::MemberRemovalAuthorityPage;
mod central_removal_companions;
pub use central_removal_companions::MemberRemovalCompanionPage;
mod central_removal_snapshots;
pub use central_removal_snapshots::DEPARTED_USER_NAME;
mod central_removal_rooms;
pub use central_removal_rooms::MemberRemovalRoomPage;
mod central_removal_effects;
pub use central_removal_effects::MemberRemovalEffectPage;
mod member_admission;
mod member_connect;
pub use member_connect::MemberConnectRoom;
mod member_projection;
mod member_resume;
pub use member_resume::{MemberResumeKey, MemberResumeKeyIssue};
mod member_schema;
pub use member_projection::MemberProjection;
mod member_sessions;
pub use member_admission::MemberAdmission;
#[cfg(test)]
mod provider_request_resolution_tests;
#[cfg(test)]
mod provider_request_tests;
mod room_file_chunk_schema;
mod room_file_content;
mod room_file_migration;
mod room_file_schema;
mod room_runtime_cleanup;
mod room_settings;
mod room_subscription;
mod room_turns;
mod room_user_identity;
mod room_votes;
mod room_write_budget;
mod schema;
mod schema_message_search;
mod schema_version;
pub use agent_memory::{AgentMemoryPiece, MemoryFold, TurnMemoryInput};
pub use schema_version::CURRENT_SCHEMA_VERSION;
mod schema_votes;
mod sqlite;
mod store_open;
mod turn_authority;
mod turn_queue;

#[cfg(test)]
mod human_invite_preflight_tests;
#[cfg(test)]
mod human_session_authority_tests;
#[cfg(test)]
mod message_attachment_tests;
mod ordered_input_route;
mod ordered_turn_release;
mod participant_leave;
mod participant_mute;
mod participant_removal;
mod participant_roles;
#[cfg(test)]
mod persistence_security_tests;
mod persona_charx;
mod persona_import;
mod persona_library;
mod persona_risu;
mod provider_turn_effect;
mod provider_turn_effect_finalize;
mod provider_turn_execution;
mod provider_turn_reconciliation;
mod provider_turn_stop;
#[cfg(test)]
mod room_appearance_asset_tests;
#[cfg(test)]
mod room_vote_tests;

pub use agent_create_start::{
    AgentCreateStartCommit, AgentCreateStartEffect, AgentCreateStartPlan,
};
pub use agent_interrupt::{AgentInterruptMutation, AgentInterruptPlan};
pub use agent_lifecycle::{
    AgentRuntimeStarted, AgentStartEffect, AgentStartPlan, AgentStopEffect, AgentStopPlan,
};
pub use agent_pause::{AgentResidentPlan, AgentResidentRuntime};
pub use agent_reconciliation::{
    LiveRuntimeReconciliation, RuntimeReconciliationCandidate, RuntimeReconciliationObservation,
    RuntimeReconciliationReservation,
};
pub use agent_reconciliation_scan::{RuntimeReconciliationCursor, RuntimeReconciliationPage};
pub use agentsassemble_domain::MAX_ATTACHMENT_BYTES;
pub use agentsassemble_domain::{
    RoomMessageContext, RoomMessageSearchPage, RoomMessageSearchResult,
};
pub use bootstrap::{LocalBootstrapCommit, LocalBootstrapPhase, LocalBootstrapStatus};
pub use guest_identity_recovery::{GuestRecoveryCommit, GuestRecoveryRequest, GuestRecoveryResult};
pub use host_identity::{PersistentHostIdentity, inspect_host_identity};
pub use human_admission::{
    HumanAdmissionCommit, HumanAdmissionDecision, HumanAdmissionInput, HumanAdmissionInputError,
    HumanAdmissionRejection, HumanAdmissionResult, PreparedHumanAdmission,
};
pub use human_invite_preflight::{
    HumanInviteCredentialEvidence, HumanInvitePreflight, HumanInvitePreflightContext,
    HumanInvitePreflightPerson, HumanInvitePreflightRejection, HumanInvitePreflightRequest,
};
pub use human_invites::{HumanInvite, NewHumanInvite};
pub use human_prejoin_attachments::HumanPrejoinAvatarAuthorization;
pub use human_session_authority::HumanSessionAuthorization;
pub use message_attachments::reader::{MessageAttachmentReadAuthority, MessageAttachmentReader};
pub use message_attachments::{
    MessageAttachment, MessageAttachmentMetadata, ProviderAttachmentReadAuthority,
};
pub use message_pins::PinnedMessage;
pub use message_search::ProviderMessageSearchAuthority;
pub use participant_leave::ParticipantLeaveMutation;
pub use participant_mute::ParticipantMuteMutation;
pub use participant_removal::ParticipantRemovalMutation;
pub use persona_charx::import_charx_asset;
pub use persona_import::{ImportedPersonaAsset, PersonaImportError, import_ccv3_asset};
pub use persona_risu::import_risum_asset;
pub use private_fs::secure_private_directory;
pub use profile_attachments::{ProfileAttachment, ProfileAttachmentMetadata};
pub use profile_store::ProfileUpdateOutcome;
pub use provider_turn_effect::{
    ProviderTurnEffectClaim, ProviderTurnEffectPhase, ProviderTurnInterruptCause,
    ProviderTurnInterruptEffect,
};
pub use provider_turn_execution::{
    ProviderTurnExecution, ProviderTurnExecutionPhase, ProviderTurnStartAuthority,
};
pub use provider_turn_reconciliation::{
    ProviderTurnReconciliationCandidate, ProviderTurnReconciliationCursor,
    ProviderTurnReconciliationPage,
};
pub use raster_assets::MAX_RASTER_DECODE_ALLOC_BYTES;
pub use room_appearance_assets::{RoomAppearanceAsset, RoomAppearanceAssetMetadata};
pub use room_directory::{RoomCreateCommit, StoredRoomSummary};
pub use room_preferences::{LocalRoomPreferencesDirectoryEntry, RoomPreferencesSnapshot};
pub use room_random::{ProviderRoomRandomCommit, RoomRandomCommit};
pub use room_runtime_cleanup::{RoomRuntimeCleanupKey, RoomRuntimeCleanupPage};
pub use room_subscription::RoomCatchUp;
pub use room_turns::{
    AgentFinalMessage, AgentTurnAssignment, AgentTurnCommit, ProviderTurnAuthority,
    RoomCommandMutation,
};
pub use room_user_identity::{LocalRoomManagerAuthority, RoomManagerAuthority, RoomUserIdentity};
pub use room_write_budget::command_size as room_write_command_size;
pub use sqlite::{
    AgentLaunchFailureCommit, CommandOutcome, PersistenceError, RoomSnapshotData, SqliteStore,
};

pub use room_turns::{AttendeeTurnOutcome, AttendeeTurnReport};

pub use attendee::turn_delivery::AttendeeTurnDelivery;
pub use provider_turn_execution::ProviderTurnAssignmentEnvelope;

#[cfg(test)]
mod member_floor_tests;

mod secure_session;
pub use secure_session::SecureSessionBinding;

#[cfg(test)]
mod backup_authority_tests;

mod connector_delivery;

mod delegated_confirmation;
mod delegated_confirmation_decision;
mod delegated_confirmation_lifecycle;

mod delegated_confirmed_agent;
mod delegated_confirmed_create;
pub use delegated_confirmed_create::{
    ApprovedCreateDrive, create_execution_payload, create_selection_payload,
};
mod delegated_confirmed_reconfigure;
pub use delegated_confirmed_reconfigure::{
    ApprovedReconfigureDrive, reconfigure_execution_payload, reconfigure_selection_payload,
};
mod delegated_confirmed_invite;
mod delegated_confirmed_kick;
pub use delegated_confirmed_invite::{
    ApprovedInviteDrive, DelegatedInvitePolicy, MintedHumanInvite, delegated_invite_policy,
};
mod delegated_confirmed_remove;
