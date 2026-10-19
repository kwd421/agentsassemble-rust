// The central session events live in their own module: the members that
// listen for them never depend on the identity module that owns the
// session, so a cleared session never depends on a module cycle.
export const CENTRAL_SESSION_CHANGED_EVENT = "agentsassemble:central-session-changed";
export const CENTRAL_SESSION_CLEARED_EVENT = "agentsassemble:central-session-cleared";
