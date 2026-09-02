import { describe, expect, it } from "vitest";
import {
  insertMentionText,
  mentionOptions,
  mentionQueryAtCursor,
} from "./mentionComposerModel";
import { roomMentionables } from "./roomMentionables";
import { agentSessionFixture } from "../test/agentSession";
import { participantFixture } from "../test/participant";

describe("roomMentionables", () => {
  it("shows one display-name option per participant instead of a second id option", () => {
    const mentionables = roomMentionables({
      displayResourceBase: "http://127.0.0.1:43123",
      viewerParticipantId: "operator-local",
      sessions: [
        agentSessionFixture({
          participant_id: "codex-codex-gpt-5.6-luna",
          display_name: "Luna — 플레이어",
          provider_kind: "codex_live_session",
        }),
      ],
      members: [
        participantFixture({
          participant_id: "operator-local",
          display_name: "SeiNel",
          participant_type: "human",
        }),
        participantFixture({
          participant_id: "codex-codex-gpt-5.6-luna",
          display_name: "Stale Participant",
          avatar_image_url: "/api/attachments/stale-avatar?view=1",
          owner_id: "operator-local",
          participant_type: "agent",
        }),
      ],
    });

    expect(mentionables).toEqual([
      {
        token: "operator-local",
        label: "SeiNel",
        avatarImage: undefined,
        participantKind: "human",
        detail: "사람",
      },
      {
        token: "codex-codex-gpt-5.6-luna",
        label: "Luna — 플레이어",
        avatarImage: undefined,
        participantKind: "agent",
        providerKind: "codex_live_session",
        detail: "SeiNel의 에이전트",
      },
    ]);
    expect(
      mentionOptions(mentionables, mentionQueryAtCursor("@luna"))[0]
    ).toMatchObject({
      token: "codex-codex-gpt-5.6-luna",
      label: "Luna — 플레이어",
      detail: "SeiNel의 에이전트",
    });
  });

  it("falls back to ids when names are absent or collide", () => {
    expect(
      roomMentionables({
        displayResourceBase: "http://127.0.0.1:43123",
        viewerParticipantId: "host",
        sessions: [
          agentSessionFixture({
            participant_id: "alpha",
            display_name: "동일 이름",
          }),
          agentSessionFixture({
            participant_id: "bravo",
            display_name: "동일 이름",
          }),
          agentSessionFixture({ participant_id: "charlie", display_name: "" }),
        ],
        members: [
          participantFixture({ participant_id: "alpha", participant_type: "agent" }),
          participantFixture({ participant_id: "bravo", participant_type: "agent" }),
          participantFixture({ participant_id: "charlie", participant_type: "agent" }),
        ],
      }).map(({ token, label }) => ({ token, label }))
    ).toEqual([
      { token: "alpha", label: "동일 이름 · alpha" },
      { token: "bravo", label: "동일 이름 · bravo" },
      { token: "charlie", label: "charlie" },
    ]);
  });

  it("inserts a participant id while preserving a spaced display label", () => {
    const expectedMention = "<@sol-dm> ";
    const mentionable = roomMentionables({
      displayResourceBase: "http://127.0.0.1:43123",
      viewerParticipantId: "host",
      sessions: [
        agentSessionFixture({
          participant_id: "sol-dm",
          display_name: "Sol — 던전 마스터",
        }),
      ],
      members: [
        participantFixture({ participant_id: "sol-dm", participant_type: "agent" }),
      ],
    })[0];

    expect(mentionable).toMatchObject({
      token: "sol-dm",
      label: "Sol — 던전 마스터",
    });
    expect(
      insertMentionText(
        "@sol",
        4,
        mentionQueryAtCursor("@sol"),
        mentionable
      )
    ).toEqual({
      message: expectedMention,
      cursor: expectedMention.length,
    });
  });

  it("takes agent presentation while keeping ownership room-owned", () => {
    const mentionable = roomMentionables({
      displayResourceBase: "",
      viewerParticipantId: "host",
      sessions: [
        agentSessionFixture({
          participant_id: "agent-1",
          display_name: "Agent One",
        }),
      ],
      members: [
        participantFixture({
          participant_id: "remote-owner",
          display_name: "Remote Owner",
          participant_type: "human",
          owner_id: "remote-owner",
        }),
        participantFixture({
          participant_id: "agent-1",
          display_name: "Participant Copy",
          owner_id: "",
          participant_type: "agent",
        }),
      ],
    }).find((entry) => entry.token === "agent-1");

    expect(mentionable).toMatchObject({
      label: "Agent One",
      detail: "에이전트",
    });
  });
});
