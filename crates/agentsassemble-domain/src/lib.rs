mod command;
mod diagnostic;
mod identity;
mod model;
mod profile;
mod projection;
mod room_preferences;
mod room_settings;
mod room_turn;
mod text;

pub use command::{CommandRejection, MessageSend, canonical_payload_hash, prepare_message_event};
pub use diagnostic::{redact_persisted_diagnostic, redact_persisted_diagnostic_text};
pub use identity::{stable_bundle_identity, stable_content_identity, stable_identity_hash};
pub use model::{
    Actor, AgentSession, AgentSessionDraft, AuthenticatedPrincipal,
    CURRENT_RUNTIME_PROFILE_VERSION, CapabilitySet, ClientKind, DurableAgentSession, InviteScope,
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, Participant, ParticipantRole,
    ParticipantStatus, ProviderAvailability, ProviderCatalog, ProviderControl,
    ProviderControlOption, Room, RoomEvent, RoomStatus, SnapshotMode,
};
pub use profile::{UserProfile, UserProfilePatch, avatar_attachment_id, canonical_avatar_url};
pub use projection::{public_event_for_principal, public_value_for_principal};
pub use room_preferences::{
    ChannelNotificationMode, ChannelPreference, MAX_PREFERENCE_CHANNELS, READ_CURSOR_LIMIT,
    RoomNotificationMode, RoomPreferencesError, RoomUserPreferences, RoomUserPreferencesPatch,
};
pub use room_settings::{
    PublicRoomSettings, RoomAppearance, RoomChannel, RoomSettings, RoomSettingsError,
    RoomSettingsPatch, public_settings,
};
pub use room_turn::{
    QueuedRoomInput, RoomInputDeliveryKind, RoomRandomError, RoomRandomRequest, RoomRandomResult,
};
pub use text::{
    clean_identifier, clean_message, clean_single_line, has_visible_text, validate_room_id,
};
