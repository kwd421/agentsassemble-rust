import { useState } from "react";
import { decideDelegatedAction, useDelegatedActionRequests } from "../../lib/delegatedActionProjection";
import { RoomSocketSayError, type RoomSocketHandle } from "../../roomSocketTypes";
import type { DelegatedActionRequest } from "../../types/generated/DelegatedActionRequest";
import DelegatedActionCard from "../components/DelegatedActionCard";

// What the owner reads is written here, from the typed action: the host's own
// consequence text is for records, not for the screen.
function describe(request: DelegatedActionRequest): { summary: string; consequence?: string; destructive?: boolean; fullAccess?: boolean } {
  const action = request.action;
  if (action.action === "delete_text_channel") {
    return { summary: `#${request.target_name} 채널을 지우려고 해요`, consequence: "채널의 메시지도 함께 지워져요.", destructive: true };
  }
  if (action.action === "start_agent") {
    const fullAccess = action.configuration.permission_mode === "full_access";
    return {
      summary: `${request.target_name} 에이전트를 시작하려고 해요`,
      consequence: fullAccess ? "이 컴퓨터 전체에 접근할 수 있는 권한으로 실행돼요." : "작업 폴더에 쓸 수 있는 권한으로 실행돼요.",
      fullAccess,
    };
  }
  if (action.action === "create_agent") {
    const { display_name, model, permission_mode, workspace } = action.draft;
    return {
      summary: `${display_name} 에이전트를 만들려고 해요`,
      consequence: [model, PERMISSION_LABELS[permission_mode] ?? permission_mode, workspace].filter(Boolean).join(" · "),
      fullAccess: permission_mode === "full_access",
    };
  }
  if (action.action === "reconfigure_agent") {
    // Only what the AI asked to change is listed; the values are the ones the host resolved.
    const { params, draft } = action;
    const changes = [
      params.model !== undefined && draft.model,
      params.reasoning_effort !== undefined && draft.reasoning_effort,
      params.permission_mode !== undefined && (PERMISSION_LABELS[draft.permission_mode] ?? draft.permission_mode),
      params.workspace !== undefined && draft.workspace,
    ].filter(Boolean);
    return {
      summary: `${draft.display_name} 에이전트 설정을 바꾸려고 해요`,
      consequence: changes.join(" · ") || undefined,
      fullAccess: draft.permission_mode === "full_access",
    };
  }
  return { summary: `${request.target_name}에 대한 작업을 하려고 해요` };
}

const PERMISSION_LABELS: Record<string, string> = {
  meeting_read_only: "대화 전용",
  workspace_write: "작업 폴더 쓰기",
  full_access: "전체 액세스",
};

function failure(error: unknown): string {
  const code = error instanceof RoomSocketSayError ? error.category : "";
  if (code === "target_changed") return "그 사이 내용이 바뀌어서 실행하지 않았어요.";
  if (code === "confirmation_expired") return "시간이 지나서 실행하지 않았어요.";
  if (code === "full_access_acknowledgement_required") return "전체 접근을 허용한다고 체크해야 해요.";
  if (code === "socket_closed" || code === "socket_not_ready") return "방 연결이 준비되지 않았어요. 잠시 후 다시 눌러 주세요.";
  return "처리하지 못했어요. 다시 눌러 주세요.";
}

/** The viewer's own pending confirmations; nobody else ever receives them. */
export default function PendingDelegatedActions({ socket, viewerParticipantId }: {
  socket: RoomSocketHandle | null;
  viewerParticipantId: string;
}) {
  const requests = useDelegatedActionRequests(socket)
    .filter((request) => request.state === "pending" && request.owner_participant_id === viewerParticipantId);
  const [busy, setBusy] = useState<Record<string, "allow" | "decline">>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  if (!socket || requests.length === 0) return null;

  async function decide(id: string, decision: "approve" | "decline", acknowledgedFullAccess = false) {
    if (!socket || busy[id]) return;
    setBusy((current) => ({ ...current, [id]: decision === "approve" ? "allow" : "decline" }));
    setErrors((current) => ({ ...current, [id]: "" }));
    try {
      await decideDelegatedAction(socket, {
        confirmation_id: id, decision,
        ...(decision === "approve" && acknowledgedFullAccess ? { full_access_acknowledged: true } : {}),
      });
    } catch (error) {
      setErrors((current) => ({ ...current, [id]: failure(error) }));
    } finally {
      setBusy((current) => { const next = { ...current }; delete next[id]; return next; });
    }
  }

  return (
    <>
      {requests.map((request) => (
        <DelegatedActionCard key={request.confirmation_id}
          delegateName={request.delegate_display_name} {...describe(request)}
          expiresAt={request.expires_at} busy={busy[request.confirmation_id]}
          error={errors[request.confirmation_id] || undefined}
          onAllow={(acknowledged) => void decide(request.confirmation_id, "approve", acknowledged)}
          onDecline={() => void decide(request.confirmation_id, "decline")} />
      ))}
    </>
  );
}
