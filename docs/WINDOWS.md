# Windows 런타임

상태: `windows-runtime-fixes` 브랜치 작업 기록. 2026-09-16, Windows 11 26200 / rustc 1.98.1 /
Node 24.13.1 에서 확인했다.

## 요약

Windows에서 런타임이 기동한 적이 없었다. 부팅 경로의 결함 세 건과, 빌드·재요청·오류 표시에서
Windows에서만 드러나는 결함 세 건을 고쳤다. 대부분 컴파일과 clippy를 통과하는 종류라 게이트에
걸리지 않았다. 데스크톱 앱이 사이드카를 띄우고 `{"status":"ready","runtime":"rust"}` 를 보고하는
것, 그리고 실제 Tauri 빌드 설정 그대로 CSS 승인 게이트를 통과하는 것까지 확인했다.

같은 PC에서 실행 중인 외부 AI를 공개 접속 없이 초대하는 기능도 이 브랜치에서 추가했다.
커넥터 복사가 URL만 나가 Grok이 HTTP로 들어오던 안내는 MCP `room_join` 경로로 바꿨다.

## 왜 드러나지 않았나

`.github/workflows/windows-contract.yml` 은 세 가지만 실행한다.

- `cargo check --locked -p agentsassemble-provider -p agentsassemble-server --all-targets`
- `cargo clippy ... -- -D warnings`
- `cargo test -p agentsassemble-provider managed_bridge`

파일 기반 데이터베이스를 여는 경로, 프론트엔드 릴리스를 만드는 경로, 데스크톱 셸을 실행하는
경로, 초대 경계 테스트는 어느 것도 실행되지 않는다. `docs/VERIFICATION.md` 의 패키지 검증은 전부
macOS 서명 빌드 기준이다.

## 고친 것

### 1. 파일 DACL을 권한 없는 핸들로 적용 (`crates/agentsassemble-persistence/src/private_fs.rs`)

증상: 서버가 기동 즉시 `writer lease operation failed: 액세스가 거부되었습니다. (os error 5)`
로 종료. 0바이트 `runtime.sqlite3` 만 남는다.

원인: `secure_file` 이 `OpenOptions::new().create_new(true).read(true).write(true)` 로 연 핸들로
DACL을 썼다. 그 핸들은 `GENERIC_READ | GENERIC_WRITE` 만 가지고 `WRITE_DAC` 가 없어
`SetSecurityInfo` 가 항상 `ERROR_ACCESS_DENIED` 를 돌려준다.

수정: 소유자 전용 DACL을 경로로 적용한다. unix 는 기존 핸들 기반 `chmod 0600` 을 그대로 쓴다.

트레이드오프: 이제 이름으로 ACL을 건다. 보호는 기존의 심링크 거부, 하드링크 카운트 검사,
`PreparedDatabase::revalidate` 의 신원 재확인에 의존한다. 이 부분은 검토가 필요하다.

### 2. 데스크톱 셸의 동일 결함 (`desktop/src-tauri/src/private_fs.rs`)

증상: 앱 창은 뜨지만 사이드카가 실행되지 않는다.

원인: 1번과 같은 결함의 복제본. 런타임 로그와 방 목록 캐시가 `WRITE_DAC` 없는 핸들로 DACL을
썼다. `secure_file_path` 를 추가하고 두 호출부가 경로를 넘긴다.

같은 보안 메커니즘이 두 크레이트에 각각 구현되어 있다는 점은 `AGENTS.md` 의 단일 소유자 규칙에
걸린다. 이 브랜치는 양쪽을 각각 고쳤을 뿐 소유자를 합치지 않았다.

### 3. 확장 경로 비교 실패 (`crates/agentsassemble-server/src/frontend_document.rs`)

증상: `frontend document cannot be bound to its release` 로 기동 실패.

원인: 릴리스 루트는 `canonicalize` 를 거쳐 Windows 에서 verbatim(`\\?\C:\...`) 경로로 오는데,
자산 경로는 URL 왕복을 거쳐 평범한 `C:\...` 로 돌아온다. 그대로 `starts_with` 로 비교해 항상
false 였다. Windows 에서만 verbatim 접두사를 제거한 뒤 비교한다.

### 4. CSS 승인 해시가 플랫폼마다 다름 (`frontend/src/views/components/RoomSyncNotice.tsx`)

증상: Windows 에서 `npm run build`, `make test`, Tauri `beforeBuildCommand` 가 항상 실패.

원인: 1527개 규칙 중 딱 하나의 부동소수점 값만 달랐다. `bg-[#29261f]/95` 를 툴체인이 계산된
`oklab()` 으로 낮추는데, 이 색은 a 채널이 0에 가까워(약 0.00056) 플랫폼 수학 라이브러리가 마지막
유효숫자를 결정한다. 깨끗한 Linux(`node:24-bookworm`) 빌드는 `.000563279`, Windows 는
`.000563338` 을 낸다. Linux 빌드가 기존 승인 해시와 정확히 일치했으므로 게이트는 정상이었고
Windows 만 달랐다. CRLF 는 원인이 아니었다.

수정: 같은 색·같은 불투명도를 정수 RGB(`bg-[rgb(41_38_31/0.95)]`)로 지정해 부동소수점 변환 없이
`#29261ff2` 로 나오게 했다. Linux 와 Windows 빌드가 모두 `index-Bo9GM-mo.css`
(`c38817be…6bba47`)를 내고, 이 값으로 승인 항목을 갱신했다. 불투명도는 0.95 에서 242/255 가 된다.

같은 종류의 색(0에 가까운 채널 + 불투명도 수식어)을 새로 쓰면 다시 플랫폼 차이가 날 수 있다.

### 5. 초대 만료 시각 정밀도 (`crates/agentsassemble-persistence/src/{connector_admission,attendee_invites}.rs`)

증상: 같은 초대를 정확히 재요청하면 `expires_at` 이 달라진다(`.057463900` → `.057463`). 초대 경계
테스트 3개가 Windows 에서 실패.

원인: DB 에는 마이크로초로 저장하면서 첫 응답에는 메모리 값을 그대로 돌려줬다. Windows 시계는
100 ns 해상도라 저장되지 않는 자릿수가 첫 응답에만 실렸다. macOS 시계는 마이크로초 단위라
드러나지 않았다. 생성 결과를 저장 코덱을 왕복한 값으로 돌려준다.

### 6. 네이티브 실패 원인이 UI에서 사라짐 (`frontend/src/views/components/StartupIdentityGate.tsx`)

증상: 네이티브 단계가 실패하면 "로컬 신원 권위를 확인하지 못했습니다" 만 표시된다.

원인: `reason instanceof Error` 일 때만 구체적 메시지를 보였는데, Tauri 커맨드는 `Result<_, String>`
의 에러 문자열로 reject 한다. 정확히 네이티브가 실패했을 때만 원인이 버려졌다. 문자열 reject 는
기본 문장 뒤에 원인을 붙인다. 네이티브 에러 문자열에는 티켓·토큰 같은 자격 증명 값이 들어가지
않는다.

### 7. 커넥터 초대가 URL만 복사되어 현재 AI가 HTTP로 입장함 (`frontend`)

증상: 로컬 초대 `http://127.0.0.1:<port>/join?token=aaci1.…` 을 Grok 대화에 붙이면, MCP
`room_join` 대신 페이지를 fetch 하고 `POST /api/room-connector/join` 으로 agent 입장을 한다.
같은 URL을 브라우저로 열면 사람 게스트 입장 UI가 뜬다.

원인: 복사 내용이 `join_url` 한 줄뿐이었다. AI 친구 초대는 `assemble room attend` 안내를 같이
복사하는데 커넥터는 URL만 줬다. 토큰 접두사 `aaci1.` 는 커넥터 초대이고, `/api/room-connector/*`
는 MCP `RoomConnectorClient` 의 HTTP 수송이다. Origin 없는 루프백 POST는 `LocalIngress` 가
허용하므로 curl 과 MCP 가 서버에서 구분되지 않는다. 초대는 1회용·1시간 능력 URL이며 agent
권한만 준다(`room_manage` 없음, 사이드챗·초대 발급 불가). HTTP join 을 막는 것은 원격 stdio MCP
도 깨뜨리므로 하지 않았다. 무제한 다회 링크는 방 비밀번호가 되므로 이번 범위가 아니다.

수정: 복사가 `connectorInviteText` 를 쓴다. MCP 등록(`assemble room connector-mcp`)과
fetch/HTTP join 금지를 포함하고, 도구 이름은 MCP 서버 지시에 맡긴다. 버튼은
"참가 안내 복사". `/join?token=aaci1.` 는 사람 게스트 join 이 아니라 같은 안내 화면
(`ConnectorJoinNotice`)이다.

## 이전 기록 정정

이 문서의 첫 판은 "`message_attachment_save/secure_replace.rs` 가 같은 핸들 결함으로 Windows 에서
첨부 저장에 실패한다"고 적었다. 실행 검증 없는 추론이었고 틀렸다. 이 경로는 Windows 에서 파일을
`access_mode(GENERIC_WRITE | WRITE_DAC)` 로 열어 처음부터 대비되어 있었고,
`atomic_save_replaces_only_the_selected_regular_path` 테스트가 Windows 에서 통과한다.
`61beeba` 커밋 메시지의 같은 문장도 틀렸다.

## 같은 PC의 외부 AI 초대

이전에는 Room Connector 초대가 공개 인그레스 없이는 불가능했다. 서버가
`public_ingress_not_ready` 로 거부했고, UI 버튼도 외부 접속이 열릴 때까지 비활성이었다.

이제 생성 요청이 `reach` 를 명시한다(`CreateConnectorInviteRequest.reach`).

| reach | join URL | 조건 |
| --- | --- | --- |
| `public` | 준비된 공개 오리진 | 공개 인그레스 준비 필요, 아니면 `public_ingress_not_ready` |
| `local` | 런타임의 루프백 리스너(`http://127.0.0.1:<port>`) | 리스너 소유자 필요, 아니면 `local_ingress_unavailable` |

서버는 reach 를 스스로 고르지 않는다. 초대 자격 증명과 입장 절차는 동일하고, 루프백 연결은
`LocalIngress` 가 이미 커넥터 라우트에 대해 인가한다. UI 는 외부 접속이 꺼져 있을 때만 `local` 을
고르고, "이 PC의 AI 초대 만들기" 버튼과 "이 PC 전용" 표시로 알린다.

로컬 링크의 제약:

- 이 PC 의 프로세스만 열 수 있다.
- 사이드카 포트는 실행마다 바뀌므로 앱을 다시 시작하면 열리지 않는다.
- 1회용, 1시간 만료는 공개 초대와 같다.

AI 친구 초대(AgentBridge, `assemble room attend`)는 여전히 공개 접속이 필요하다.

Agent Session(앱이 provider 를 직접 실행)은 이 경로와 무관하게 원래부터 인그레스가 필요 없다.

### Grok 으로 연결하기

Room Connector 는 이미 실행 중인 AI 대화가 MCP 로 방에 들어오는 방식이다. 초대 URL을
열거나 `/api/room-connector/join` 에 POST 하지 않는다. UI 복사는 MCP 등록 안내와 URL을
같이 넣고, 도구 이름은 MCP 서버 지시에 맡긴다.

```
# 전역 설정을 건드리지 않도록 전용 폴더의 project 범위에 등록한다
cd <작업 폴더>
grok mcp add --scope project agentsassemble <repo>\target\debug\assemble.exe -- room connector-mcp
grok --trust mcp doctor    # 14 tools discovered 확인
grok                       # 폴더 신뢰 후, 복사한 참가 안내를 전달
```

프로젝트 MCP는 폴더가 trusted 여야 기동한다. `grok mcp doctor` 가
`folder untrusted (repo-local (project-scoped) server not started for an untrusted folder)`
이면 `--trust` 가 필요하다. 이 작업 폴더는 처음에 untrusted였고 `grok-room` 만 trusted였다.
`grok --trust mcp doctor` 로 핸드셰이크와 도구 14개를 확인했다. 이미 열린 Grok 세션은 MCP를
추가해도 도구가 안 붙을 수 있다. `/mcps` 에서 `r` 이거나 새 세션이 필요하다.

## Windows 에서 실행하기

```
npm --prefix frontend install
npm --prefix provider-runtime ci --omit=optional --ignore-scripts
npm --prefix desktop install
npm --prefix desktop run prepare:sidecar
npx --prefix desktop tauri build --debug --no-bundle

# 리소스를 실행 파일 옆에 배치한 뒤 실행
#    target/debug/frontend/          <- frontend/dist 내용
#    target/debug/provider-runtime/  <- provider-runtime 의 .mjs 와 sdk.mjs
target/debug/agentsassemble-desktop.exe
```

CSS 게이트가 고쳐졌으므로 설정 덮어쓰기 없이 실제 `beforeBuildCommand` 로 빌드된다.

`tauri dev` 는 쓰지 말 것. dev 서버가 `http://127.0.0.1:1430` 에서 UI 를 서빙하는데,
`caller_is_bundled_ui` 는 `tauri://localhost` 와 `tauri.localhost` 만 허용하므로 모든 네이티브
커맨드가 거부된다. macOS 에서도 동일하다.

## 검증 범위

실행한 것:

- 데스크톱 앱 기동, 사이드카 ready, `/healthz` · `/app/` 200
- 실제 Tauri 빌드에서 CSS 승인 게이트 통과, Linux 컨테이너 빌드와 해시 일치
- 프론트엔드 전체 테스트 893개 (병렬 부하에서 타임아웃 난 1개는 단독 실행 시 통과)
- `human_invite_manager_boundary` 7개, 첨부 저장 테스트 4개
- 패키지 앱에서 외부 접속이 꺼진 상태로 "이 PC의 AI 초대 만들기" 버튼 활성 확인
- Grok 의 커넥터 MCP 기동과 도구 노출
- 로컬 초대 URL만 받은 Grok 이 MCP 없이 HTTP join 하는 경로 재현
- `useConnectorInvites` · `roomDockModel` 관련 프론트 테스트 14개 (안내 복사, `aaci1.` 은 사람 게스트가 아님)

실행하지 않은 것:

- 서버·persistence 크레이트 전체 테스트
- MCP `room_join` 으로 같은 Grok 세션이 로컬 방에 다시 입장하는 전 과정 (이 세션에는 도구가 안 붙음)
- 에이전트 턴, 공개 인그레스(cloudflared) 전체 경로
- 롤링 재시작. `runtime_reexec::InheritedListeners` 는 `cfg(unix)` 전용이다.
