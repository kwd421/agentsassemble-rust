import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { canonicalParticipantProfiles, applyCanonicalParticipantProfiles, applyParticipantEvents } from "../../lib/canonicalRoomProjection";
import { participantFixture } from "../../test/participant";
import type { LobbyEvent } from "../../api";
import MessageRow from "./MessageRow";
import MemberRow from "./member/MemberRow";
import { Bot } from "lucide-react";

afterEach(cleanup);
it("uses the same saved human label in members and messages despite an agent-like room role", () => {
  const member = participantFixture({ participant_type: "human", role: "director", display_name: "Human", avatar_label: "XY" });
  const event = { id: "m", actor_id: member.participant_id, actor_type: "human", name: "Human", kind: "message", side: "other", message: "hello", created_at: member.created_at } as LobbyEvent;
  const updated = applyParticipantEvents([member], [{ v: 1, id: "update", seq: 1, created_at: member.created_at, room_id: member.room_id, actor: { participant_id: member.participant_id, participant_type: "human" }, type: "participant_updated", participant_id: member.participant_id, avatar_label: "ZZ" }]);
  const projected = applyCanonicalParticipantProfiles([event], canonicalParticipantProfiles([], updated, ""))[0];
  const { container } = render(<>
    <MemberRow entry={{ id: member.participant_id, member: updated[0], displayName: "Human", detail: "", role: "director", owner: true, active: true, muted: false, meetingId: "general", icon: Bot }} onOpenDetails={() => {}} onRoleChange={() => {}} onContextMenu={() => {}} canEditRoles={false} />
    <MessageRow eventId="m" author={projected.name} createdAt={projected.created_at} participantType={projected.actor_type} avatarLabel={projected.avatar_label}>hello</MessageRow>
  </>);
  expect(container.querySelector(".dc-member-avatar")?.textContent).toBe("ZZ");
  expect(container.querySelector(".dc-message-avatar")?.textContent).toBe("ZZ");
  expect(container.querySelector(".lucide-bot")).toBeNull();
});
it("retains photo precedence and agent/system icons", () => {
  const { container } = render(<>
    <MessageRow eventId="h" author="Human" createdAt="" participantType="human" avatarLabel="XY" avatarImage="/avatar.png">photo</MessageRow>
    <MessageRow eventId="a" author="Agent" createdAt="" participantType="agent">agent</MessageRow>
    <MessageRow eventId="s" author="Room" createdAt="" system>system</MessageRow>
  </>);
  expect(container.querySelector("img")?.getAttribute("src")).toBe("/avatar.png");
  expect(container.querySelector(".lucide-bot")).not.toBeNull();
  expect(container.querySelector(".lucide-zap")).not.toBeNull();
});
