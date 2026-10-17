import { describe, expect, it } from "vitest";
import { formatDelegatedActionText, objectParticle, subjectParticle } from "./delegatedActionText";
import type { LobbyEvent } from "../api";

const sources = {
  participantProfiles: {
    "agent-1": { displayName: "claude-haiku-5-5" },
    "owner-1": { displayName: "ㅏㄴ나나" },
    "target-1": { displayName: "새내기" },
    "consonant-1": { displayName: "한글" },
  },
  channelLabels: { "ch-1": "기획", "ch-2": "공지" },
};

function line(action: string, message: string, overrides: Partial<NonNullable<LobbyEvent["delegation"]>> = {}) {
  return formatDelegatedActionText(
    {
      name: "claude-haiku-5-5",
      message,
      delegation: {
        action,
        targetId: "ch-1",
        onBehalfOfParticipantId: "owner-1",
        actorParticipantId: "agent-1",
        ...overrides,
      },
    },
    sources
  );
}

describe("formatDelegatedActionText", () => {
  it("renders a channel creation from the structured fields instead of the English record", () => {
    const text = line(
      "create_text_channel",
      "claude-haiku-5-5 updated a text channel on behalf of ㅏㄴ나나."
    );
    expect(text).toContain("claude-haiku-5-5");
    expect(text).toContain("ㅏㄴ나나");
    expect(text).toContain("기획");
    expect(text).not.toContain("on behalf of");
    expect(text).not.toContain("updated a text channel");
  });

  it("covers every channel action the server emits", () => {
    expect(line("rename_text_channel", "x renamed a text channel on behalf of y.")).toContain("기획");
    expect(line("delete_text_channel", "x deleted a text channel and its messages on behalf of y.")).toContain("기획");
  });

  it("covers pin and unpin through the structured flag", () => {
    const pinned = line("set_message_pin", "x pinned a message on behalf of y.",
      { targetId: "msg-1", pinned: true, targetChannelId: "ch-2" });
    expect(pinned).toContain("claude-haiku-5-5");
    expect(pinned).toContain("공지");
    const unpinned = line("set_message_pin", "x unpinned a message on behalf of y.",
      { targetId: "msg-1", pinned: false });
    expect(unpinned).not.toBe(pinned);
    expect(unpinned).toContain("ㅏㄴ나나");
  });

  it("covers mute and unmute through the structured flag", () => {
    const muted = line("set_participant_mute", "x muted a participant on behalf of y.", { targetId: "target-1", muted: true });
    const unmuted = line("set_participant_mute", "x unmuted a participant on behalf of y.", { targetId: "target-1", muted: false });
    expect(muted).toContain("새내기");
    expect(unmuted).toContain("새내기");
    expect(unmuted).not.toBe(muted);
  });

  it("uses the structured mute flag even when a display name contains unmuted", () => {
    const mutedSources = {
      participantProfiles: {
        "agent-1": { displayName: "진행자" },
        "owner-1": { displayName: "방주인" },
        "target-1": { displayName: "UnmutedUser" },
      },
      channelLabels: {},
    };
    const text = formatDelegatedActionText(
      {
        name: "진행자",
        message: "x muted UnmutedUser on behalf of y.",
        delegation: {
          action: "set_participant_mute",
          targetId: "target-1",
          onBehalfOfParticipantId: "owner-1",
          actorParticipantId: "agent-1",
          muted: true,
        },
      },
      mutedSources
    );
    expect(text).toContain("음소거했어요");
    expect(text).not.toContain("해제");
  });

  it("stays neutral without the structured mute flag (old hosts)", () => {
    const text = line("set_participant_mute", "x muted a participant on behalf of y.", { targetId: "target-1" });
    expect(text).toContain("음소거 상태");
  });

  it("covers agent start, stop, create, reconfigure and remove", () => {
    for (const action of ["start_agent", "stop_agent", "create_agent", "reconfigure_agent", "remove_agent"]) {
      const text = line(action, `x ${action} on behalf of y.`, { targetId: "target-1" });
      expect(text).toContain("claude-haiku-5-5");
      expect(text).toContain("새내기");
      expect(text).not.toContain("on behalf of");
    }
  });

  it("covers kick and human invite", () => {
    const kicked = line("kick_participant", "x kicked 새내기 on behalf of y.", { targetId: "target-1" });
    expect(kicked).toContain("새내기");
    const invite = line("create_human_invite", "x created a room invite on behalf of y.", { targetId: "" });
    expect(invite).toContain("claude-haiku-5-5");
    expect(invite).toContain("ㅏㄴ나나");
  });

  it("falls back to a neutral phrase when names are unknown", () => {
    const text = formatDelegatedActionText(
      {
        name: "",
        message: "x updated a text channel on behalf of y.",
        delegation: {
          action: "create_text_channel", targetId: "missing-channel",
          onBehalfOfParticipantId: "missing-owner", actorParticipantId: "missing-actor",
        },
      },
      sources
    );
    expect(text).not.toContain("on behalf of");
    expect(text).not.toContain("missing-channel");
    expect(text).toContain("채널");
  });

  it("does not double the noun when the channel name is unknown", () => {
    for (const [action, verb] of [
      ["create_text_channel", "만들었어요"],
      ["rename_text_channel", "이름을 바꿨어요"],
      ["delete_text_channel", "삭제했어요"],
    ] as const) {
      const text = line(action, `x ${action} on behalf of y.`, { targetId: "missing-channel" });
      expect(text).toContain("채널");
      expect(text).toContain(verb);
      expect(text).not.toContain("채널 채널");
    }
    const known = line("delete_text_channel", "x deleted a text channel on behalf of y.");
    expect(known).toContain("#기획 채널을");
    const deletedUnknown = line("delete_text_channel", "x deleted a text channel on behalf of y.",
      { targetId: "missing-channel" });
    expect(deletedUnknown).toContain("채널을 삭제했어요");
  });

  it("does not double the noun when the agent or participant name is unknown", () => {
    for (const action of ["start_agent", "stop_agent", "create_agent", "remove_agent"]) {
      const text = line(action, `x ${action} on behalf of y.`, { targetId: "missing-agent" });
      expect(text).toContain("에이전트를");
      expect(text).not.toContain("에이전트 에이전트");
    }
    const reconfigured = line("reconfigure_agent", "x reconfigure on behalf of y.", { targetId: "missing-agent" });
    expect(reconfigured).toContain("에이전트 설정을 바꿨어요");
    expect(reconfigured).not.toContain("에이전트 에이전트");
    const knownAgent = line("start_agent", "x start on behalf of y.", { targetId: "target-1" });
    expect(knownAgent).toContain("새내기 에이전트를");
    const muted = line("set_participant_mute", "x muted a participant on behalf of y.", { targetId: "missing-agent", muted: true });
    expect(muted).toContain("참가자를 음소거했어요");
    expect(muted).not.toContain("참가자 참가자");
    const kicked = line("kick_participant", "x kicked someone on behalf of y.", { targetId: "missing-agent" });
    expect(kicked).toContain("참가자를 내보냈어요");
    expect(kicked).not.toContain("참가자 참가자");
    const pinned = line("set_message_pin", "x pinned a message on behalf of y.",
      { targetId: "msg-1", pinned: true, targetChannelId: "missing-channel" });
    expect(pinned).toContain("메시지를 고정했어요");
    expect(pinned).not.toContain("채널 채널");
  });

  it("falls back to a neutral phrase for an unknown action", () => {
    const text = line("future_action", "x did something on behalf of y.");
    expect(text).toContain("claude-haiku-5-5");
    expect(text).not.toContain("future_action");
  });

  it("returns null without structured fields so other system lines keep their text", () => {
    expect(formatDelegatedActionText({ name: "x", message: "차례 넘김" }, sources)).toBeNull();
  });

  it("selects particles from the final syllable", () => {
    expect(subjectParticle("한글")).toBe("이");
    expect(subjectParticle("나나")).toBe("가");
    expect(subjectParticle("claude-haiku-5-5")).toBe("가");
    expect(objectParticle("한글")).toBe("을");
    expect(objectParticle("새내기")).toBe("를");
    const text = line("kick_participant", "x kicked 한글 on behalf of y.", { targetId: "consonant-1" });
    expect(text).toContain("한글을");
  });
});
