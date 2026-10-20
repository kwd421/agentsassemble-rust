import type { ReactNode } from "react";
import React from "react";
import ReactDOM from "react-dom/client";
import { Bot } from "./ui/icons";
import { agentSessionFixture } from "./test/agentSession";
import AgentProfileCard from "./views/components/member/AgentProfileCard";
import MemberRow from "./views/components/member/MemberRow";
import type { MemberEntry } from "./views/components/member/memberTypes";
import "./index.css";

const ULTRA_SESSION = agentSessionFixture({
  room_id: "uishot-room",
  session_id: "uishot-session",
  participant_id: "uishot-agent",
  display_name: "제미나이 라이트",
  status: "available",
  runtime_status: "idle",
  enabled: true,
  provider_kind: "antigravity",
  runtime_kind: "antigravity_cli",
  connection_kind: "antigravity_cli",
  execution_harness: "external_cli",
  model: "Gemini 3 Pro",
  reasoning_effort: "ultra",
});

const ULTRA_ENTRY: MemberEntry = {
  id: ULTRA_SESSION.participant_id,
  agentSession: ULTRA_SESSION,
  displayName: ULTRA_SESSION.display_name,
  detail: "Gemini 3 Pro",
  modelLabel: "Gemini 3 Pro",
  reasoningEffort: "ultra",
  ultraMode: true,
  statusLabel: "대기 중",
  role: "agent",
  owner: false,
  providerKind: ULTRA_SESSION.provider_kind,
  active: true,
  muted: false,
  meetingId: ULTRA_SESSION.room_id,
  icon: Bot,
};

const FLEX_SESSION = agentSessionFixture({
  room_id: "uishot-room",
  session_id: "uishot-flex",
  participant_id: "uishot-flex-agent",
  display_name: "클로드 소넷",
  status: "available",
  runtime_status: "idle",
  enabled: true,
  provider_kind: "opencode",
  runtime_kind: "opencode",
  connection_kind: "opencode",
  execution_harness: "external_cli",
  model: "Claude Sonnet 4.5",
  reasoning_effort: "medium",
  service_tier: "flex",
});

const FLEX_ENTRY: MemberEntry = {
  id: FLEX_SESSION.participant_id,
  agentSession: FLEX_SESSION,
  displayName: FLEX_SESSION.display_name,
  detail: "Claude Sonnet 4.5",
  modelLabel: "Claude Sonnet 4.5",
  reasoningEffort: "medium",
  speed: "flex",
  statusLabel: "대기 중",
  role: "agent",
  owner: false,
  providerKind: FLEX_SESSION.provider_kind,
  active: true,
  muted: false,
  meetingId: FLEX_SESSION.room_id,
  icon: Bot,
};

function GalaxyUishot() {
  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-6">
      <header>
        <p className="text-xs font-bold uppercase tracking-[0.18em] text-[#8b8d96]">
          UI capture · galaxy
        </p>
        <h1 className="mt-1 text-xl font-black text-[#f2f3f5]">
          최대 추론 에이전트의 은하
        </h1>
      </header>

      <section
        className="rounded-xl border border-white/10 p-5"
        style={{ background: "#1e1f22" }}
      >
        <p className="mb-4 text-xs font-bold uppercase tracking-[0.14em] text-[#8b8d96]">
          멤버 목록 · 32px 아바타 · 15px 이름 · 은하 행 · Flex 코인
        </p>
        <div style={{ width: 280 }}>
          <MemberRow
            entry={ULTRA_ENTRY}
            onOpenDetails={() => {}}
            onContextMenu={() => {}}
            canEditRoles={false}
            canManageParticipant={false}
          />
          <MemberRow
            entry={FLEX_ENTRY}
            onOpenDetails={() => {}}
            onContextMenu={() => {}}
            canEditRoles={false}
            canManageParticipant={false}
          />
        </div>
      </section>

      <section
        className="rounded-xl border border-white/10 p-5"
        style={{ background: "#2b2d31" }}
      >
        <p className="mb-4 text-xs font-bold uppercase tracking-[0.14em] text-[#8b8d96]">
          프로필 헤더 · 48px 아바타 · 카메라 버튼 위
        </p>
        <div className="dc-member-detail-modal" style={{ width: 420 }}>
          <AgentProfileCard
            session={ULTRA_SESSION}
            detail="Gemini · Gemini 3 Pro"
            onSave={async () => {}}
            onAvatarUpdate={async () => {}}
          >
            {null}
          </AgentProfileCard>
        </div>
      </section>
    </div>
  );
}

const UI_SHOT_STATES: Record<string, () => ReactNode> = {
  galaxy: GalaxyUishot,
};

function UishotStates() {
  const requested =
    new URLSearchParams(window.location.search).get("state") || "galaxy";
  const State = UI_SHOT_STATES[requested] || UI_SHOT_STATES.galaxy;
  return (
    <main className="min-h-screen bg-[#1e1f22] p-8 text-[#f2f3f5]">
      <State />
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <UishotStates />
  </React.StrictMode>
);
