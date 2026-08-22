import type { PublicRoomSettings } from "./generated/PublicRoomSettings";

export type PublicRoomGlobalSettings = PublicRoomSettings;

export interface PublicProviderRequest {
  provider_request_id?: string;
  participant_id?: string;
  display_name?: string;
  provider_kind?: string;
  request_kind?: string;
  status?: string;
  title?: string;
  description?: string;
}
