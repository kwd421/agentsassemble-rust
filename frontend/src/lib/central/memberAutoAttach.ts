import { previewSecureMember, SecureMemberAdmission, type SecureMemberEntry } from "./secureMemberEntry";
import { loadMemberAnchor, saveMemberAnchor, type MemberHostAnchor } from "./memberAnchors";
import type { CentralServer } from "./identity";
import { probeMemberHostOrigin, type MemberHostOriginProbeResult } from "./memberHostOriginProbe";

export type MemberAttachDeps = {
  preview: (entry: SecureMemberEntry, signal?: AbortSignal) => ReturnType<typeof previewSecureMember>;
  open: (entry: SecureMemberEntry, target: Parameters<typeof SecureMemberAdmission.open>[1], sessionToken: string, signal: AbortSignal) => Promise<SecureMemberAdmission>;
  probe?: (origin: string, expected: { server_id: string; host_key_fingerprint: string }) => Promise<MemberHostOriginProbeResult>;
};

const liveDeps: MemberAttachDeps = {
  preview: previewSecureMember,
  open: (entry, target, sessionToken, signal) => SecureMemberAdmission.open(entry, target, sessionToken, signal),
  probe: (origin, expected) => probeMemberHostOrigin(origin, expected),
};

export class MemberAnchorMismatchError extends Error {
  constructor() {
    super("참가 서버 정보가 바뀌었어요. 새 초대로 다시 참가해 주세요.");
    this.name = "MemberAnchorMismatchError";
  }
}

/**
 * Silent re-entry to one joined member host: the stored origin and this
 * device's resume key are tried first, so a reconnect costs no central
 * request at all. A host that cannot resume, an expired key or an unknown
 * key falls back once to today's central path.
 *
 * The anchor pins host IDENTITY only (server_id, registration_epoch, host
 * key fingerprint); the endpoint origin/generation is taken fresh from the
 * central directory on every attach, so a friend's new random public origin
 * re-attaches silently. Throws MemberAnchorMismatchError only on identity
 * change (epoch or key fingerprint; the stored anchor is kept so the known
 * rooms stay visible as disconnected with the re-join reason until fresh
 * consent overwrites it), and the preview/host error otherwise.
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
  // Before spending central: when the anchor carries a stored origin, probe
  // it directly without central (unsigned GET <origin>/api/server-info).
  // Same host answering there attaches with the stored identity and the
  // probed public key — only the preview step is skipped; the secure
  // admission still takes its central grant through the existing path.
  // A silent probe miss (down / moved / foreign host) falls through to the
  // single preview below, still gated by the slow schedule outside.
  const probeHit = stored && deps.probe && stored.endpoint_origin
    ? await deps.probe(stored.endpoint_origin, {
        server_id: stored.server_id,
        host_key_fingerprint: stored.host_key_fingerprint,
      }).catch(() => null)
    : null;
  if (probeHit && stored) {
    const directTarget = {
      server_id: stored.server_id,
      registration_epoch: stored.registration_epoch,
      origin: stored.endpoint_origin,
      generation: stored.endpoint_generation,
      host_key_fingerprint: probeHit.host_key_fingerprint,
      host_public_key_jwk: probeHit.host_public_key_jwk,
    };
    let admission: SecureMemberAdmission | undefined;
    try {
      signal.throwIfAborted();
      // The handshake verifies the pinned host key before anything is signed.
      admission = await deps.open(entry, directTarget as never, sessionToken, signal);
      // Central-free first: this device's own resume key proves it on this
      // same channel. A host that cannot resume it, an expired key or an
      // unknown key falls back to the central grant on the same channel.
      const direct = await resumedRooms(admission, signal);
      // The connect grant below spends central: an aborted flight never
      // reaches it.
      signal.throwIfAborted();
      const rooms = direct ?? (await admission.admit(signal));
      if (!rooms) throw new Error("참가 중인 방을 확인하지 못했어요.");
      const anchor: MemberHostAnchor = {
        server_id: directTarget.server_id,
        registration_epoch: directTarget.registration_epoch,
        host_key_fingerprint: directTarget.host_key_fingerprint,
        endpoint_origin: directTarget.origin,
        endpoint_generation: directTarget.generation,
        rooms: rooms.map((room) => ({ room_id: room.room_id, name: room.name })),
        hidden: stored?.hidden ?? false,
        updated_at: Date.now(),
      };
      saveMemberAnchor(personId, anchor);
      return { admission, rooms, anchor };
    } catch (error) {
      // A stale generation (same origin republished) fails the handshake
      // here with no central spent yet; fall through to the single preview
      // below so the fresh generation is picked up. A down host between
      // probe and open lands on the same preview path as a probe miss.
      if (signal.aborted) throw error;
      detachMemberHost(admission);
    }
  }
  // The select deadline owns this signal: a hung central preview aborts with
  // the select instead of stranding the rail selection forever.
  // An already-aborted flight never spends central: StrictMode remount (or an
  // identity flap) aborts the first flight while its attach still runs.
  signal.throwIfAborted();
  const preview = await deps.preview(entry, signal);
  // The preview itself already spent central; an abort while it was in
  // flight still stops the transport open and the connect grant below.
  signal.throwIfAborted();
  const target = preview.target;
  if (stored && stored.host_key_fingerprint !== target.host_key_fingerprint) {
    throw new MemberAnchorMismatchError();
  }
  const admission = await deps.open(entry, target, sessionToken, signal);
  try {
    // The connect grant inside admit spends central: an aborted flight never
    // reaches it.
    signal.throwIfAborted();
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

/**
 * The rooms a resumed admission lists, or null when this device cannot
 * resume on this host. Duck-typed so plain test doubles with the same shape
 * behave like the real admission; anything else is treated as "no direct
 * path" and the central path takes over.
 */
async function resumedRooms(
  admission: SecureMemberAdmission,
  signal?: AbortSignal,
): Promise<{ room_id: string; name: string }[] | null> {
  const resume = (admission as {
    resumeDirect?: (signal?: AbortSignal) => Promise<{ room_id: string; name: string }[]>;
  }).resumeDirect;
  if (typeof resume !== "function") return null;
  try {
    return await resume.call(admission, signal);
  } catch {
    return null;
  }
}
