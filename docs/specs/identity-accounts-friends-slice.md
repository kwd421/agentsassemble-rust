# Identity, accounts, friends and human admission

Status: Phase 5 active after Phase 4 approval at `53a82f1`.

## Definition and dependency order

Complete the retained account, friend and human admission flows discovered in the
original reachable account/social/invite routes, native login return broker and
frontend callers. Existing Rust human admission, profile, recovery and local
bootstrap behavior remains authoritative after cutover. Do not restore v0 meeting
or agent-research code, Freebuff, or managed Antigravity.

First export browser-device credential and human-session bearer wire constants
from their distinct protocol domains (C-13 human portion). Generation, parsing,
fingerprinting, authorization and boundary-specific errors stay with their current
owners. Audit every human signed-invite claim for a finite production consumer;
remove fixed self-description whose only consumer repeats its literal (D-07).
Preserve checked scope, expiry, target room, ingress and durable credential binding.
No compatibility parser or migration of old user data is introduced.

Then connect the phase-wide owners and one complete vertical flow for each target:

- Local/public account status, Google challenge, verified connection and disconnect.
  The server persists links to the existing human profile and binds proof to the
  requesting identity/device. A Google account already linked to another identity
  requires the original explicit guest-discard decision and an atomic server-owned
  switch. A paired operator cannot create durable account authority.
- Central Google native handoff through the existing central identity service and
  PKCE flow. The native host permits only the exact Google authorization surface;
  the local return owner binds state, bounded capacity/expiry, cancellation and
  returned code without persisting or logging credentials. Central guest creation,
  bootstrap and recovery remain separate from local server account links.
- Durable saved friends and their reachable add/edit/delete/filter/invite flow.
  A saved contact is not a room admission grant or proof of live provider status.
  Preserve supplied provider identity and distinguish human invites from external
  AI admission, whose execution belongs to Phase 7.
- Operator-pairing create/redeem/revoke bound to an exact room incarnation, current
  local host, ready ingress origin, device and short expiry. Atomic redemption
  prevents duplicate use and cannot upgrade an ordinary human invite. Revocation,
  room closure and departure use existing session/lifecycle owners. Local host
  claim stays with current bootstrap rather than a parallel browser host identity.
- Human invitation presentation, preflight/join/recovery/leave and errors continue
  through the existing canonical admission and retained retry owners. Do not merge
  Human Invite, Room Connector and AgentBridge credentials or lifecycle state.

## Authority, failures and verification

`SqliteStore` remains the durable writer; current HTTP admission and local-purpose
control tickets remain transport boundaries. Add only necessary identity/account/
friend/pairing records at the clean current schema. Revalidate mutable authority in
its committing transaction, publish room changes through the existing event owner,
and retain exact retry semantics where an external effect or admission can commit
before the response arrives. Do not expose success for uncertain saves or expired
proof, invent an account from an invalid token, or use browser state as authority.

Use maintained libraries for OAuth/JWT infrastructure and the existing bounded
HTTP client and task-cancellation owners. Any callback polling must have the real
browser-return lifetime, a fixed bound and termination on success/cancel/expiry;
no new background heartbeat is justified by this phase.

Acceptance covers every visible account/friend/invite control with complete server
behavior and no absent startup route. Verify authorization, wrong identity/origin/
room, replay/conflict, expiry/revoke, rollback and restart at the affected boundaries;
reuse current profile/admission tests. Directly operate the packaged app at desktop
and 390px widths through open/edit/cancel/save/error and invite flows, preserving the
UX guide's margins, focused dialogs and concise menus. Run affected tests and all
unchanged structure/security/CSS gates, measure phase resource costs and obtain
Daybreak whole-phase approval before Phase 6. Real providers and the final Pro
review remain deferred by user instruction; local fixtures do not prove those runs.
