import { previewSecureMember, SecureMemberAdmission, type SecureMemberEntry } from "./secureMemberEntry";
import { loadMemberAnchor, saveMemberAnchor, type MemberHostAnchor } from "./memberAnchors";
import type { CentralServer } from "./identity";

export type MemberAttachDeps = {
  preview: (entry: SecureMemberEntry, signal?: AbortSignal) => ReturnType<typeof previewSecureMember>;
  open: (entry: SecureMemberEntry, target: Parameters<typeof SecureMemberAdmission.open>[1], sessionToken: string, signal: AbortSignal) => Promise<SecureMemberAdmission>;
};

const liveDeps: MemberAttachDeps = {
  preview: previewSecureMember,
  open: (entry, target, sessionToken, signal) => SecureMemberAdmission.open(entry, target, sessionToken, signal),
};

export class MemberAnchorMismatchError extends Error {
  constructor() {
    super("참가 서버 정보가 바뀌었어요. 새 초대로 다시 참가해 주세요.");
    this.name = "MemberAnchorMismatchError";
  }
}

/**
 * Silent re-entry to one joined member host: central preview pins the live
 * incarnation, then a fresh member-connect grant admits through the existing
 * challenge/redeem/select path. Bearers stay memory-only; only the display
 * anchor persists. The anchor pins host IDENTITY only (server_id,
 * registration_epoch, host key fingerprint); the endpoint origin/generation
 * is taken fresh from the central directory on every attach, so a friend's
 * new random public origin re-attaches silently. Throws
 * MemberAnchorMismatchError only on identity change (epoch or key
 * fingerprint; the stored anchor is kept so the known rooms stay visible
 * as disconnected with the re-join reason until fresh consent overwrites
 * it), and the preview/host error otherwise.
 */
export async function attachMemberHost(
  server: Pick<CentralServer, "server_id" | "registration_epoch">,
  sessionToken: string,
  personId: string,
  signal: AbortSignal,
  deps: MemberAttachDeps = liveDeps,
): Promise<{ admission: SecureMemberAdmission; rooms: { room_id: string; name: string }[]; anchor: MemberHostAnchor }> {
  if (!server.registration_epoch) throw new Error("서버 등록 정보를 확인하지 못했어요.");
  const stored = loadMemberAnchor(personId, server.server_id);
  if (stored && (stored.registration_epoch !== server.registration_epoch)) {
    throw new MemberAnchorMismatchError();
  }
  const entry: SecureMemberEntry = { server_id: server.server_id, registration_epoch: server.registration_epoch };
  // The select deadline owns this signal: a hung central preview aborts with
  // the select instead of stranding the rail selection forever.
  const preview = await deps.preview(entry, signal);
  const target = preview.target;
  if (stored && stored.host_key_fingerprint !== target.host_key_fingerprint) {
    throw new MemberAnchorMismatchError();
  }
  const admission = await deps.open(entry, target, sessionToken, signal);
  try {
    const rooms = await admission.admit(signal);
    if (!rooms) throw new Error("참가 중인 방을 확인하지 못했어요.");
    const anchor: MemberHostAnchor = {
      server_id: target.server_id,
      registration_epoch: target.registration_epoch,
      host_key_fingerprint: target.host_key_fingerprint,
      endpoint_origin: target.origin,
      endpoint_generation: target.generation,
      rooms: rooms.map((room) => ({ room_id: room.room_id, name: room.name })),
      hidden: stored?.hidden ?? false,
      updated_at: Date.now(),
    };
    saveMemberAnchor(personId, anchor);
    return { admission, rooms, anchor };
  } catch (error) {
    admission.close();
    throw error;
  }
}

export function detachMemberHost(admission: SecureMemberAdmission | null | undefined): void {
  try { admission?.close(); } catch { /* transport close is best-effort */ }
}
