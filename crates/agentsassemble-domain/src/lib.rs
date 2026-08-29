mod asset;
mod command;
mod diagnostic;
mod identity;
mod message_attachments;
mod message_pins;
mod model;
mod profile;
mod projection;
mod room_preferences;
mod room_settings;
mod room_turn;
mod text;

pub use asset::MAX_ATTACHMENT_BYTES;
pub use command::{
    CommandRejection, MessageSend, canonical_payload_hash, prepare_message_event,
    require_message_write_authority,
};
pub use diagnostic::{redact_persisted_diagnostic, redact_persisted_diagnostic_text};
pub use identity::{stable_bundle_identity, stable_content_identity, stable_identity_hash};
pub use message_attachments::{
    MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES, MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS,
    MAX_MESSAGE_ATTACHMENTS_PER_EVENT, MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX,
    MESSAGE_ATTACHMENT_ID_HEX_LENGTH, MESSAGE_ATTACHMENT_ID_PREFIX,
    MESSAGE_ATTACHMENT_REFERENCE_PREFIX, MESSAGE_ATTACHMENT_VIEW_SUFFIX,
    canonical_message_attachment_filename, is_message_attachment_id,
};
pub use message_pins::{
    MAX_LOBBY_MESSAGE_PINS, MAX_MESSAGE_PIN_EVENT_ID_BYTES, is_message_pin_event_id,
};
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
    PublicRoomSettings, ROOM_APPEARANCE_ASSET_HEX_LENGTH, ROOM_APPEARANCE_ASSET_PREFIX,
    ROOM_APPEARANCE_REFERENCE_PREFIX, ROOM_APPEARANCE_REFERENCE_QUERY,
    ROOM_APPEARANCE_REFERENCE_SUFFIX, RoomAppearance, RoomChannel, RoomSettings, RoomSettingsError,
    RoomSettingsPatch, is_room_appearance_asset_id, public_settings, room_appearance_asset_id,
};
pub use room_turn::{
    QueuedRoomInput, RoomInputDeliveryKind, RoomRandomError, RoomRandomRequest, RoomRandomResult,
};
pub use text::{
    clean_identifier, clean_message, clean_single_line, has_visible_text, validate_room_id,
};
