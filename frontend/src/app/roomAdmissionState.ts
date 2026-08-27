import { GUEST_SESSION_EXPIRED_MESSAGE } from "../lib/apiErrors";
import {
  roomGuestSessionExpired,
  type RoomGuestSession,
} from "../lib/roomGuestSession";

export const SERVER_SURFACE_INVALID_CODE = "server_surface_invalid";
export const SESSION_CUSTODY_INVALID_CODE = "session_storage_unavailable";

export type AdmissionSource =
  | "initial"
  | "invite"
  | "pairing"
  | "existing_session"
  | "recovery";
export type AdmissionOperation = "preflight" | "join" | "intent_cleanup" | "pairing";

export type AdmissionState =
  | { kind: "idle"; session: null; status: "" }
  | { kind: "preflighting"; session: RoomGuestSession | null; status: string }
  | { kind: "profile_required"; session: RoomGuestSession | null; status: "" }
  | { kind: "joining"; session: RoomGuestSession | null; status: string }
  | {
      kind: "joined";
      session: RoomGuestSession;
      source: AdmissionSource;
      status: "";
    }
  | { kind: "pairing"; session: RoomGuestSession | null; status: string }
  | {
      kind: "failed";
      session: RoomGuestSession | null;
      operation: AdmissionOperation;
      code: string;
      message: string;
      retryable: boolean;
      status: string;
    }
  | { kind: "expired"; session: null; status: string };

export type AdmissionAction =
  | { type: "preflight_started"; status: string }
  | { type: "profile_required" }
  | { type: "join_requested"; status: string }
  | { type: "pairing_started"; status: string }
  | { type: "joined"; session: RoomGuestSession; source: AdmissionSource }
  | {
      type: "failed";
      operation: AdmissionOperation;
      code: string;
      message: string;
      retryable: boolean;
      status: string;
    }
  | { type: "expired"; status: string }
  | { type: "session_surface_failed"; message: string }
  | { type: "session_cleared" };

export function initialAdmissionState({
  guestJoinToken,
  operatorPairingToken,
  initialSession,
}: {
  guestJoinToken: string;
  operatorPairingToken: string;
  initialSession: RoomGuestSession | null;
}): AdmissionState {
  if (operatorPairingToken) {
    return { kind: "pairing", session: initialSession, status: "" };
  }
  if (guestJoinToken) {
    return {
      kind: "preflighting",
      session:
        initialSession && !roomGuestSessionExpired(initialSession)
          ? initialSession
          : null,
      status: "",
    };
  }
  if (initialSession) {
    if (roomGuestSessionExpired(initialSession)) {
      return { kind: "expired", session: null, status: GUEST_SESSION_EXPIRED_MESSAGE };
    }
    return { kind: "joined", session: initialSession, source: "initial", status: "" };
  }
  return { kind: "idle", session: null, status: "" };
}

export function admissionReducer(
  state: AdmissionState,
  action: AdmissionAction
): AdmissionState {
  switch (action.type) {
    case "preflight_started":
      if (
        state.kind !== "preflighting" &&
        !(state.kind === "failed" && state.operation === "preflight" && state.retryable)
      ) {
        return state;
      }
      return { kind: "preflighting", session: state.session, status: action.status };
    case "profile_required":
      return { kind: "profile_required", session: state.session, status: "" };
    case "join_requested":
      if (
        state.kind !== "preflighting" &&
        state.kind !== "profile_required" &&
        state.kind !== "joining" &&
        !(state.kind === "failed" && state.operation === "join" && state.retryable)
      ) {
        return state;
      }
      return { kind: "joining", session: state.session, status: action.status };
    case "pairing_started":
      if (
        state.kind !== "pairing" &&
        !(state.kind === "failed" && state.operation === "pairing" && state.retryable)
      ) {
        return state;
      }
      return { kind: "pairing", session: state.session, status: action.status };
    case "joined":
      return { kind: "joined", session: action.session, source: action.source, status: "" };
    case "failed":
      return {
        kind: "failed",
        session: state.session,
        operation: action.operation,
        code: action.code,
        message: action.message,
        retryable: action.retryable,
        status: action.status,
      };
    case "expired":
      return { kind: "expired", session: null, status: action.status };
    case "session_surface_failed":
      return {
        kind: "failed",
        session: null,
        operation: "join",
        code: SERVER_SURFACE_INVALID_CODE,
        message: action.message,
        retryable: false,
        status: action.message,
      };
    case "session_cleared":
      if (state.kind === "idle" || state.kind === "expired") return state;
      if (state.kind === "joined") return { kind: "idle", session: null, status: "" };
      return { ...state, session: null };
  }
}
