import { normalizeRoomGlobalSettings } from "../api/room";
import { describe, expect, it } from "vitest";
import {
  commandAckResultIsValid,
  publicRoomEventIsValid,
} from "./roomSocketValidation";

function settings(label = "General") {
  return {
    settings_revision: "settings-general",
    label,
    topic: "",
    appearance: {
      banner_preset: "default",
      banner_image_url: "",
      icon_image_url: "",
      icon_label: "G",
      invite_scope: "room",
    },
    conversation_mode: "ordered",
    tool_mode: "chat",
    ordered_exclude_previous_speaker: true,
    channels: [],
  };
}

function result(eventSettings: unknown, resultSettings: unknown) {
  return {
    room_settings: resultSettings,
    event: {
      v: 1,
      id: "settings-event-1",
      seq: 1,
      created_at: "2026-08-25T00:00:01Z",
      room_id: "general",
      type: "room_settings_updated",
      actor: { participant_id: "operator-local", participant_type: "human" },
      room_settings: eventSettings,
    },
    event_seq: 1,
  };
}

describe("room settings command ACK validation", () => {
  it("accepts one exact generated settings projection", () => {
    const projection = settings();
    expect(commandAckResultIsValid(
      "room.settings.update",
      {},
      result(projection, projection),
      "general",
      "operator-local",
    )).toBe(true);
  });

  it("rejects conflicting settings with the same revision", () => {
    expect(commandAckResultIsValid(
      "room.settings.update",
      {},
      result(settings("Event label"), settings("Result label")),
      "general",
      "operator-local",
    )).toBe(false);
  });
});

describe("room settings event validation", () => {
  it("accepts the exact generated settings projection", () => {
    expect(publicRoomEventIsValid(
      result(settings(), settings()).event,
      "general",
    )).toBe(true);
  });

  it.each(["missing field", "extra field"] as const)(
    "rejects a %s in a standalone settings event",
    (conflict) => {
      const projection = settings() as Record<string, unknown>;
      if (conflict === "missing field") delete projection.tool_mode;
      else projection.legacy_label = "General";
      expect(publicRoomEventIsValid(
        result(projection, settings()).event,
        "general",
      )).toBe(false);
    },
  );
});


it("carries a new room lobby name and rejects a mismatched acknowledgement", () => {
  const named = { ...settings(), lobby_name: "팀 대화" };
  expect(commandAckResultIsValid("room.settings.update", {}, result(named, named), "general", "operator-local")).toBe(true);
  expect(normalizeRoomGlobalSettings(named, "general")?.lobbyName).toBe(named.lobby_name);
  expect(normalizeRoomGlobalSettings(settings(), "general")?.lobbyName).toBeUndefined();
  expect(commandAckResultIsValid("room.settings.update", {}, result(named, settings()), "general", "operator-local")).toBe(false);
  for (const lobby_name of [null, 1, ""]) {
    const invalid = { ...named, lobby_name };
    expect(publicRoomEventIsValid(result(invalid, invalid).event, "general")).toBe(false);
  }
});


it("carries room file policy and rejects malformed or conflicting projections", () => {
  const policy = { ...settings(), attachment_file_limit_bytes: 26214400, attachment_storage_limit_bytes: 524288000 };
  expect(commandAckResultIsValid("room.settings.update", {}, result(policy, policy), "general", "operator-local")).toBe(true);
  expect(normalizeRoomGlobalSettings(policy, "general")).toMatchObject({ attachmentFileLimitBytes: 26214400, attachmentStorageLimitBytes: 524288000 });
  expect(commandAckResultIsValid("room.settings.update", {}, result(policy, { ...policy, attachment_storage_limit_bytes: null }), "general", "operator-local")).toBe(false);
  for (const attachment_file_limit_bytes of [null, "26214400", 26214400.5, 1]) {
    const invalid = { ...policy, attachment_file_limit_bytes };
    expect(normalizeRoomGlobalSettings(invalid, "general")).toBeNull();
    expect(publicRoomEventIsValid(result(invalid, invalid).event, "general")).toBe(false);
  }
});
