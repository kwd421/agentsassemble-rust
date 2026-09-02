import { describe, expect, it } from "vitest";
import { commandAckResultIsValid } from "./roomSocketValidation";

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
    activity_plugin: "",
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
