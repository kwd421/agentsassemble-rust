import type { RoomSayRequest } from "../roomSocketTypes";
import { RoomSocketSayError } from "../roomSocketTypes";
import { MAX_MESSAGE_ATTACHMENTS_PER_EVENT } from "../types/generated/MESSAGE_ATTACHMENTS_WIRE";
import { messageAttachmentId } from "./messageAttachmentId";

function attachmentIds(request: RoomSayRequest): string[] {
  const attachments = request.attachments || [];
  if (attachments.length > MAX_MESSAGE_ATTACHMENTS_PER_EVENT) {
    throw new RoomSocketSayError(
      `A room message cannot contain more than ${MAX_MESSAGE_ATTACHMENTS_PER_EVENT} attachments.`,
      "bad_request"
    );
  }
  let ids: string[];
  try {
    ids = attachments.map((attachment) => messageAttachmentId(attachment.id));
  } catch {
    throw new RoomSocketSayError(
      "A room message contains an invalid attachment identifier.",
      "bad_request"
    );
  }
  if (new Set(ids).size !== ids.length) {
    throw new RoomSocketSayError(
      "A room message cannot contain duplicate attachments.",
      "bad_request"
    );
  }
  return ids;
}

function withAttachments(
  payload: Record<string, unknown>,
  request: RoomSayRequest
): Record<string, unknown> {
  const ids = attachmentIds(request);
  return ids.length ? { ...payload, attachment_ids: ids } : payload;
}

/** Maps one copied composer request to the exact Rust-owned message.send payload. */
export function roomMessagePayload(request: RoomSayRequest): Record<string, unknown> {
  const kind = request.kind || "message";
  switch (kind) {
    case "message":
      return withAttachments({ content: request.message }, request);
    case "vote":
      return withAttachments({
        kind,
        vote_question: request.voteQuestion,
        vote_options: request.voteOptions,
        ...(request.voteDurationSeconds === undefined
          ? {}
          : { vote_duration_seconds: request.voteDurationSeconds }),
      }, request);
    case "vote_cast":
      return { kind, vote_id: request.voteId, vote_choice: request.voteChoice };
    case "vote_withdraw":
    case "vote_close":
      return { kind, vote_id: request.voteId };
    default:
      throw new RoomSocketSayError(
        `Room message kind ${String(kind)} is unsupported.`,
        "bad_request"
      );
  }
}
