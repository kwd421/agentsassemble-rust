import { act, renderHook } from "@testing-library/react";
import { expect, it } from "vitest";
import type { RoomEvent } from "../api";
import { DEPARTED_USER_NAME, PARTICIPANT_ANONYMIZED_EVENT_TYPE } from "../types/generated/PARTICIPANT_ANONYMIZATION_WIRE";
import { applyCanonicalParticipantProfiles } from "./canonicalRoomProjection";
import { anonymizedParticipant, scrubHistoricalAuthors, useAnonymousAuthors } from "./participantAnonymization";
import { projectRoomEventsToTimeline } from "./roomEventProjection";
import { publicRoomEventIsValid } from "./roomSocketValidation";

const anonymous: RoomEvent = { v: 1, id: "anonymization", seq: 3, created_at: "2026-10-10T00:00:00Z", room_id: "general",
  type: PARTICIPANT_ANONYMIZED_EVENT_TYPE, actor: { participant_id: "departing", participant_type: "human" },
  participant_id: "departing", participant_type: "human", actor_id: "departing", actor_type: "human",
  display_name: DEPARTED_USER_NAME, avatar_image_url: "", avatar_label: "" };
const message: RoomEvent = { v: 1, id: "message", seq: 1, created_at: anonymous.created_at, room_id: "general",
  type: "message_final", actor: anonymous.actor, display_name: "Private Name", avatar_image_url: "https://example.com/private.png", content: "Private Name is in the message text", message_kind: "message" };

it("rewrites loaded history, keeps author overrides after event eviction, and fences another workspace", () => {
  const hook = renderHook(({ scope }) => useAnonymousAuthors(scope), { initialProps: { scope: "host-one" } });
  act(() => { hook.result.current.apply("general", [anonymous]); });
  // The durable event may leave the bounded window; late history still resolves the same author.
  act(() => { hook.result.current.apply("general", [message]); });
  const overrides = hook.result.current.authors.general;
  const scrubbed = scrubHistoricalAuthors([message], overrides);
  expect(scrubbed[0].content).toBe(message.content);
  expect(scrubbed[0].display_name).toBe(DEPARTED_USER_NAME);
  expect(scrubbed[0].avatar_image_url).toBe("");
  const timeline = projectRoomEventsToTimeline([message], { participantProfiles: overrides });
  expect(timeline[0].name).toBe(DEPARTED_USER_NAME);
  expect(timeline[0].avatar_image_url).toBeUndefined();
  expect(applyCanonicalParticipantProfiles(timeline, overrides)[0].name).toBe(DEPARTED_USER_NAME);
  hook.rerender({ scope: "host-two" });
  expect(hook.result.current.authors).toEqual({});
  expect(projectRoomEventsToTimeline([message])[0].name).toBe("Private Name");
});

it("rejects identity mismatches and retained photos/profile snapshots", () => {
  expect(publicRoomEventIsValid(anonymous, "general")).toBe(true);
  for (const invalid of [{ ...anonymous, avatar_image_url: "old-photo" },
    { ...anonymous, display_name: "old-name" }, { ...anonymous, actor_id: "other" },
    { ...anonymous, profile: { display_name: "Private Name" } }]) {
    expect(() => anonymizedParticipant(invalid)).toThrow();
    expect(publicRoomEventIsValid(invalid, "general")).toBe(false);
  }
});
