# Room lifecycle and moderation

Status: Phase 4 approved by Daybreak at `53a82f1`, C0/H0/M0/L0.

## 중앙 member의 방-local/서버 종료 — C1 승인 계약 (2026-10-05)

### 파일 지도

- `crates/agentsassemble-persistence/src/participant_leave.rs`, `participant_removal.rs`, `participant_rows.rs`, `participant_roles.rs`, `participant_mute.rs`: 기존 방-local 상태·revision·제거·작업 취소 권위.
- `crates/agentsassemble-persistence/src/room_lifecycle.rs` (`revoke_room_access`), `room_deletion.rs`: room access generation·committed 폐기·삭제 recovery.
- `crates/agentsassemble-persistence/src/server_owner_authority.rs`: server-wide kick/ban/unban의 유일한 `ServerOwnerAuthority`.
- `crates/agentsassemble-persistence/src/server_memberships.rs`, `member_sessions.rs`, `member_admission_results.rs`, `membership_outbox.rs` (모두 예정): membership CAS·전 기기 폐기·결과/receipt·종료 통지; 기존 제거 소유자에 결합.
- `crates/agentsassemble-server/src/central_member_web.rs` (예정): `member.leave` 및 제한된 결과조회/서버 명령 경계.
- `crates/agentsassemble-server/src/room_socket.rs`, `room_socket_session.rs`, `room_agent_lifecycle_runtime.rs`: live transport 종료와 기존 durable 작업 정리 후처리.
- `frontend/src/views/components/member/ParticipantRemovalControls.tsx`, `frontend/src/views/components/CentralServerList.tsx`: 기존 방-local 제거와 중앙 목록의 동일 host 탈퇴 명령; 공통 앱/웹 UI.
- [Identity C1](identity-accounts-friends-slice.md#초대-멤버를-중앙-계정에-묶기--c1-승인-계약-2026-10-05)는 binding/membership/enrollment/삭제/floor/공통 예산/C2–C6, [invite C1](human-invite-admission-session-slice.md#중앙-member-입장과-세션--c1-승인-계약-2026-10-05)은 entitlement/parent/child를 소유한다.

### 요구 동작과 기존 participant 제거 진입점

승인 설계 5판+보완의 구현 전 계약이다. 기존 `participant.leave/kick/export`는 **방-local**이다. 기존 권한·범위를 보존하고 방 moderator/member/operator를 서버 소유자로 승격하지 않는다. A방 제거는 A의 participant/revision·진행 작업·child/ticket만 종료하고 B방·server membership·다른 방을 위한 parent 권위를 변경하지 않는다.

기존 human/member status/role/mute/join/leave는 동일 participant 행을 사용한다. canonical local-user binding과 단조 revision을 추가하고 변경 시 해당 사람의 해당 방 전 기기 child/ticket을 폐기한다. 종료 revision은 durable 취소가 아직 완료되지 않았어도 진행 작업의 추가 적용을 차단한다. entitlement에는 participant 상태를 복제하지 않는다.

### Server leave/kick/ban/unban과 durable 결과

진입은 live parent의 본인 전용 `member.leave`, 별도 server-member kick/ban/unban 명령이다. 후자는 **ServerOwnerAuthority만** 실행하며 방 moderator·member·operator로 대체하지 않는다. 중앙 목록 탈퇴도 동일 host 명령이고 오프라인 성공을 표시하지 않는다.

| 명령 | membership 전이 (revision CAS) | 기존 participant 종료 |
| --- | --- | --- |
| leave | active→left | Joined→Left |
| kick | active→left | Joined→Kicked |
| ban | active 또는 left→banned | Joined→Kicked |
| unban | banned→left | 기존 비활성 상태 보존; 재활성화 없음 |

Binding당 한 membership 행을 유지한다. 기존 제거 소유자가 membership CAS와 해당 사람의 **모든 방** participant 종료를 한 트랜잭션에서 조정한다. 기존 비활성 participant는 보존한다. revision·방별 roster/권한 이벤트·서버 이벤트·durable 작업 취소·전 기기 parent/child/ticket 폐기·결과·outbox를 함께 확정한다. 방-local 종료와 달리 server leave/kick/ban은 모든 방 접근과 진행 작업을 종료한다. 외부 작업 완료를 추정하지 않고 기존 취소/정리 소유자가 후처리한다.

모든 서버 명령은 request ID·actor·대상·payload hash에 결합한 durable 결과를 갖는다. 같은 요청은 같은 결과, 같은 ID의 다른 입력은 conflict다. 새 요청은 현재 권위/CAS를 검사한다. 방 moderator의 서버 명령은 거절한다. 포화 상태에서도 종료/폐기·예약 결과 확정은 가능해야 하며 admission 때 종료 통지/receipt 슬롯을 예약한다.

### Leave 응답 유실과 제한된 결과조회

기존 명령 결과 저장을 확장하여 leave 결과를 `(binding_id, request_id, payload hash, source parent fingerprint)`에 결합한다. 폐기 parent의 해시·결과·조회 기한은 일반 parent 정리와 분리해 예약한다. 폐기된 **정확한 source parent**는 제한된 결과조회 endpoint에서 같은 binding/request/payload의 저장 결과만 읽을 수 있다. 새 명령·다른 결과·child 발급은 금지한다. 중앙 없이 재시작 후에도 조회할 수 있어야 한다.

Leave 결과와 source-parent 증명은 **커밋부터 24시간** 보존한다. 클라이언트는 전송 전에 request ID/입력을 저장한다. 기한 이후 조회 불가는 “결과 확인 불가”이며 실패로 추정하지 않는다. receipt의 별도 행/byte/전역 예산은 공통 예산 계약대로 노출 전에 확정하며 일반 종료 기록의 600초 정리/child TTL 300초를 receipt에 적용하지 않는다.

### 기존 revoke_room_access의 room generation 폐기

Archive/close/delete의 기존 `revoke_room_access`는 같은 트랜잭션에서 room access generation을 증가시키고 member child/ticket을 폐기하여 **committed fingerprint/generation**을 반환한다. 기존 lifecycle 후처리는 해당 generation의 live transport를 종료한다. 읽기·WS frame·HTTP mutation·발급을 폐기와 직렬화하여 종료 뒤 stale 접근을 막는다. 응답 유실·재시작·deletion recovery도 같은 committed 폐기를 재실행한다.

Parent는 다른 방을 위해 유지할 수 있다. restore/unarchive 후에도 옛 child/ticket은 부활하지 않는다. 새 발급만 현재 방 상태·membership·Joined participant·canonical entitlement·revision/generation을 검사하여 허용한다. 중앙 account/server 삭제는 이 방 lifecycle이나 host 탈퇴 성공을 대신하지 않으며 중앙 provenance/늦은 결과 처리는 identity 소유 계약을 따른다.

### 제외한 후속 요구, 순서와 수용 기준

첫 슬라이스에서는 새 로컬 사람·기존 binding 재입장만 제공하며 익명 합치기·scope 변경·reapproval은 제외한다. 일반 invite/re-entry는 Left/Kicked/left/banned를 복구하지 않는다. 후속 reapproval은 person/binding·예상 revision·새 invite·방별 목표·만료에 결합한 **단일 소비 owner 승인**으로 명시된 방만 기존 participant 소유자가 복구한다. 후속 scope 변경은 admission 소유자의 명시적 owner CAS와 해당 방 전 기기 child 폐기가 필요하다. bound guest-discard·guest 전환은 retirement 소유자가 변경 전에 거절한다. 계정 탈퇴도 binding/removal fence를 보존하며 전환을 승인하는 중앙 ACK를 만들지 않는다. 후속 bound-to-guest 전환은 이 슬라이스 밖의 별도 소유자 승인 권위 설계가 필요하다.

Identity C1의 공통 상한과 C2–C6가 적용된다: C2 limiter/owner 격리·grant 재사용·bounded cleanup 완료, C3 중앙 호환 floor와 구 Worker 차단 장벽/내부 경로, C4a 별도 host floor, C4b migration 및 이 소유자 내부 연결, C5 완전 수직 검증 후 앱/웹 함께 노출, C6 혼재·실제 두 기기·구조/실행 게이트·계획 소유자 리뷰다. 과거 host schema 80/member 81 예약·81 인식 floor는 역사적 v80/v81 rollout이며 현재 schema 지시가 아니다. 현재 계정 탈퇴는 identity 계약의 직접 확인한 v86 → 다음 미사용 lifecycle schema upgrade를 따른다. 적용된 migration을 재번호화하지 않고 frozen binding DDL를 보존한다. member migration 후 floor 이전 rollback은 금지하고 floor rollback에서도 member 폐기 권위를 보존한다. native pairing idle 수명/last-use/30일 미사용 만료/단일·전체 revocation을 모든 단계에서 보존한다.

필수 테스트/수용 기준:

- 방 moderator의 서버 명령 거절; A방 leave/kick/export가 B방·membership을 보존함; owner 서버 종료가 A/B roster·revision·작업·HTTP/WS/읽기·전 기기를 함께 종료함을 barrier로 검증한다.
- Archive/close/delete와 child 발급·읽기·송신 경쟁; committed fingerprint/generation·응답 유실·재시작·deletion recovery의 같은 폐기; 다른 방 parent 유지; restore 뒤 옛 child/ticket 비부활을 검증한다.
- Leave 커밋 직후 응답 유실·중앙 오프라인·재시작에서 정확한 폐기 parent 결과조회 성공, 다른 request/payload/parent 및 새 명령 거절, 24시간 보존/정리, 중복 서버 명령의 단일 효과를 검증한다.
- Left/Kicked/banned 일반 복귀 금지, 권한 downgrade 경쟁과 추가 작업 적용 차단, server CAS/동시 admission·unban 비복구·room-local 격리, 포화 때 종료/receipt 가능을 검증한다. 후속 reapproval 단일 소비/명시 방 한정을 보존한다. bound guest-discard·guest 전환은 무변경 거절되며 guest나 recovery credential을 만들지 않음을 검증한다.
- Identity의 예약/삭제·unknown/floor 테스트와 invite의 parent 수명/상한 테스트를 함께 통과하고 실제 앱/웹에서 기기 A 초대→기기 B 목록 재입장→owner 서버 kick으로 양쪽 차단을 직접 확인한다. C1은 계약만이며 이 실행 수용을 완료했다고 주장하지 않는다.

## Contract and dependency order

Retain original canonical participant kick/export and room close/archive/delete,
including their reachable frontend or bounded HTTP entry points. The room owner
controls deletion and must supply the current room name. Room moderation remains
server-authorized; a target cannot replace a principal or remove the local owner.
Host-device claim belongs to the existing Rust local bootstrap authority rather
than a second browser-token identity. Verify that entry point as part of this phase.

First consolidate the repeated participant row codec (C-01). One connection-scoped
load-by-key decodes an optional row; one exact save checks the affected row count.
Transactions, missing-row policy, identity/authority checks, state transitions and
canonical events remain at their existing owners. Preserve query keys and rollback
behavior. Do not introduce a generic repository or cache.

Before more settings, remove the future-only `activity_plugin` field (C-09): its
only frontend consumer is the explicitly deferred RimWorld surface. Keep that
surface unreachable. Use an explicit clean schema boundary without migrating user
data. Export the authoritative 128-character room label limit to its existing UI
(F-19); preserve existing channel/profile ownership and server validation.

Then connect moderation and room lifecycle to the existing command, durable replay,
provider runtime custody, human-session revocation and room directory owners.
Kick/export removes exact membership/access, stops or retains explicit unresolved
custody for the exact Agent runtime, and publishes the canonical removed state.
Close prevents further writes/admission and cleans owned running work and access;
archive/unarchive preserves data and an authenticated management path. Archive revokes
live access and stops owned work while preserving membership and settings; restoring
requires completed cleanup and never reopens a closed room. Delete
requires exact-name confirmation and a durable result/tombstone so retry cannot
retarget a recreated room. Cleanup failure must remain visible and recoverable;
never claim runtime termination or deletion from only a submitted effect.

Removal commits access revocation before external cleanup. A pending cleanup row
references the existing Agent Session rather than copying its runtime identity or
creating another provider supervisor. It fences new launch/re-add effects until
the existing exact runtime/turn reconciliation owners prove absence. The current
server recovery watcher consumes bounded pending cleanup work; no second timer is
introduced. Removed membership survives normal stop and recovery. Room rows and
owned assets remain present while cleanup is unresolved; physical deletion follows
confirmed cleanup and retains an exact command tombstone outside the room cascade.

Positive runtime absence must remain durable even when it precedes moderation.
Packaged archive-after-restart exposed the old Gone transition clearing custody
while retaining disconnected/recovery-required state. The reconciliation owner
must checkpoint stopped/no-recovery before clearing that custody, so a subsequent
archive, kick or delete can complete without re-observing an erased identity.
Ambiguous observations retain their exact identity and recovery fence. No inference
from empty fields, old error text or an external process PID repairs prior records.
Verify the ordinary start → confirmed shutdown → database reopen → archive/restore
sequence, alongside the existing uncertain-absence negative case and packaged flow.

Deletion records one exact request and the terminal closed event before cleanup.
Until completion its HTTP resolution is unresolved, retaining the same retry intent.
The existing recovery watcher finalizes bounded pending deletions after the close
event has entered canonical publication and all runtime cleanup rows are gone.
Only that transaction deletes room-owned rows/assets and commits the immutable
success result outside the room cascade. Replay authenticates the current local
bootstrap/profile owner and the stored request/hash/UID, without requiring deleted
membership or touching a new incarnation. No request can revise pending deletion;
the closed room cannot be restored. No new timer or filesystem asset owner is needed.

Lifecycle management uses the authenticated HTTP directory boundary because archive
and close invalidate ordinary room admission, and restoration must work without a
room socket. Both transports delegate to the same bounded room command owner; the
HTTP response is not the broadcast authority. Existing sequence-coupled commands
and all real-time updates remain WebSocket-owned. The user's 2026-09-07 transport
clarification prioritizes stability and replaceable transport adapters, not a
wholesale Discord-protocol copy. No duplicate HTTP moderation wrapper is retained
without a distinct integration consumer.

No client orchestration substitutes for server lifecycle. No real providers or
user-room deletion runs during implementation. No new fallback, gate exception,
plugin framework, scheduler, or periodic cleanup is implied by this phase.

## Acceptance and verification

### Shared owner recovery after room retirement (2026-10-03)

The signed Mac flow exposed unresolved deletion custody disappearing with the
room settings modal. The next room's dialog offered an unexplained retry for the
previous room, and an empty workspace had no retry entrance. The web owner also
used a room session that deletion revokes, leaving terminal recovery native-only.

Preserve one exact lifecycle intent and show its original target, progress,
failure and retry in the shared shell even after switching or removing the last
room. Only the canonical committed receipt clears pending custody; directory
absence is not completion. Do not poll, auto-resubmit or create another request.
Server-wide web owners use their existing redeemed, device/origin/generation-bound
grant for lifecycle admission, queued mutation and retained deletion replay.
Revalidate it in the transaction that reads or mutates the receipt, derive the
canonical host principal on the server and retain command budgets, queues,
incarnation/name checks, publication and cleanup ownership. Ordinary paired
sessions stay room-scoped; they do not inherit server ownership or terminal replay.
Native one-use tickets retain their current contract. Cross-client retirement can
clear a room session before the directory removes its old row. Automatic selection
must not re-enter that just-ended incarnation; select another canonical room when
available. A retired room is a room conflict, not invalid server-owner authentication.

Verify the existing uncertain-request case with an empty directory and unchanged
retry identity, HTTP grant rejection and archive/restore/delete/terminal replay,
then signed Mac and Chrome last-room deletion, visible retry and subsequent room
creation/deletion. Preserve unrelated data, providers and Claude's composition.

- Existing participant mutation, admission, profile, lifecycle and recovery tests
  preserve their public results with the shared codec; missing exact save fails and
  transaction rollback remains observable.
- Room label hints derive from the server limit; unsupported plugin state is absent
  from live schema, wire and UI projections.
- Each advertised moderation/lifecycle action has exact principal/target checks,
  canonical ACK/events, deterministic replay/conflict and restart recovery. Guest,
  Agent Bridge, wrong-room, stale incarnation and owner-removal attempts fail.
- Runtime cleanup is owned by the existing server runtime lifecycle; in-flight
  turns, reservations, human sessions, invite/ticket authority and asset custody
  cannot survive room/participant removal with usable authority.
- Desktop/mobile settings and roster controls report failure, reflect events and
  reconnect, and expose only implemented capabilities. Preserve archive management
  and deletion replay without weakening active-room admission for ordinary clients.
- Run affected local TCP/WebSocket, persistence and frontend proof plus unchanged
  architecture, source, format, Clippy and CSS gates. Obtain whole-phase Daybreak
  approval before Phase 5. Direct packaged app manipulation is required before the
  phase review, including earlier Phase 2 controls and Phase 3 profile UI. Real-provider
  execution remains at final closeout.
