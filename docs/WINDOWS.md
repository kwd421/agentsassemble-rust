# Windows 런타임

상태: `windows-runtime-fixes` 브랜치 작업 기록. 2026-09-16, Windows 11 26200 / rustc 1.98.1 /
Node 24.13.1 에서 확인했다.

## 요약

Windows에서 런타임이 기동한 적이 없었다. 이 문서는 그 과정에서 **관찰한 증상, 코드에서 찾은
원인, 적용한 수정, 실제로 확인한 범위**를 적는다. 적은 것보다 넓은 범위가 정상이라고 주장하지
않는다. 다루는 것은 부팅 경로 세 건, 빌드·재요청·오류 표시 세 건, provider 실행 관련 여덟 건이며,
대부분 컴파일과 clippy 를 통과하는 종류라 기존 게이트에 걸리지 않았다.

같은 PC에서 실행 중인 외부 AI를 공개 접속 없이 초대하는 기능과, 없는 provider CLI 를 확인 후
설치하는 기능도 이 브랜치에서 추가했다. 커넥터 복사가 URL만 나가 Grok이 HTTP로 들어오던 안내는
MCP `room_join` 경로로 바꿨다.

한 가지 한계를 먼저 적는다. 이 작업은 모두 한 대의 Windows 11 PC 에서, 이 계정의 설치 상태
(npm 전역 설치, `HOME` 없음, Claude · OpenCode 데스크톱 앱)를 기준으로 확인했다. 다른 설치
방식이나 다른 계정 환경에서 같은 결론이 나오는지는 확인하지 않았다.

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

### 8. 에이전트 생성이 Windows에서 프로바이더마다 실패함

증상: 에이전트 추가 창에서 프로바이더가 쓸 수 없는 상태로 보였다. Grok 은
`The original provider start did not complete before server recovery.`, Codex 는
`provider model discovery failed`, Claude · OpenCode · Cursor 는 `configured command missing`.

원인은 하나가 아니었다. 코드를 읽고 이 PC 의 환경과 대조해 아래처럼 판단했으며, 세 가지 모두
이번 브랜치의 부팅 수정이 만든 회귀가 아니라 부팅이 가능해지면서 드러난 기존 코드의 문제로 보인다.

#### 8-1. Grok 이 기존 로그인을 넘겨받지 못하는 경로 (`grok_acp.rs`, `grok.rs`)

세션마다 전용 `GROK_HOME` 을 만들고 기존 로그인은 `GROK_AUTH_PATH` 로 따로 넘기는 구조인데,
그 경로를 찾는 코드가 `GROK_AUTH_PATH` → `GROK_HOME` → `HOME/.grok` 만 확인했다. Windows 는
`HOME` 을 설정하지 않으므로(이 PC 에서 프로세스 · 사용자 · 시스템 수준 모두 없음) 로그인해 둔
`%USERPROFILE%\.grok\auth.json` 이 있어도 결과가 `None` 이었다. 빈 전용 홈과 자격 증명 없는
상태로 `grok agent … stdio` 가 실행되고, attach 실패가 복구 경로를 거쳐
`runtime_start_recovered_gone` 문구로 끝났다.

두 호출부가 하나의 해석기를 쓰도록 바꿨다. `GROK_HOME`, 없으면 플랫폼 홈 아래 `.grok`
(Windows 는 `USERPROFILE`, Codex 설정이 이미 쓰던 방식). 세션 격리는 그대로다.

수정 뒤 패키지 앱에서 기존 Grok 세션을 시작하면 `grok agent … stdio` 자식 프로세스가 유지되고
복구 문구 없이 대기 상태로 들어갔다. 다만 수정 전후의 실패 지점을 런타임 로그로 직접 비교하지는
않았으므로, 이 변경이 그 증상의 유일한 원인이었다고 단정하지는 않는다.

#### 8-2. Codex 가 npm 의 Windows 래퍼를 네이티브 실행 파일로 오인함 (`codex_executable.rs`)

Windows 의 PATHEXT 탐색은 확장자 붙은 후보만 보므로 npm 전역 설치에서 `codex.cmd` 가 선택된다.
해석기는 첫 두 바이트가 `#!` 가 아니면 네이티브로 보고 `.cmd` **옆에서** 동반 실행 파일
(`codex-code-mode-host.exe`)을 찾다가 실패했다. 실제 번들은
`<prefix>\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\…\bin\` 에 있다.

이제 `.cmd` 항목은 npm 이 셸 안에 적어 두는 `"%dp0%\<상대 경로>.js"` 를 따라가 패키지 스크립트로
해석하고, 기존 스크립트 경로가 네이티브 번들을 찾는다. 상대 경로의 일반 세그먼트만 허용하며 동반
파일 검증은 그대로다. npm 전역 · 프로젝트 로컬 설치를 덮고, 다른 패키지 매니저의 래퍼는 아니다.

#### 8-3. Claude · OpenCode · Cursor 는 실제로 PATH 에 없었다

이 PC 에서 `claude`, `opencode`, `cursor-agent` 는 PATH 에 없다. 대신 Claude 는 Claude 데스크톱
앱이, OpenCode 는 OpenCode 데스크톱 앱이 각자 번들로 설치해 두었다. Claude 데스크톱은 MSIX 앱이라
그 파일이 `%APPDATA%\Claude\...` 로 보이지만 실제로는 앱 전용 저장소
(`%LOCALAPPDATA%\Packages\Claude_<게시자>\LocalCache\...`)에 있고, 다른 프로그램은 그 경로로
접근하지 못한다. 즉 `configured command missing` 은 정확한 보고였다.

이 상태를 사용자가 해결할 수 있도록 in-app 설치를 추가했다(아래). Cursor 는 npm 배포가 아니라
기존 공식 안내만 유지한다. Cursor 는 `cursor-agent` 가 PATH 에 없다는 것까지만 확인했고, 다른
위치에 설치되어 있는지는 확인하지 않았다.

#### 8-4. Codex 의 in-app 업데이트가 Windows 에서 비활성 (`provider_updater.rs`)

npm 업데이트는 실행 전에 "지금 찾은 런처가 npm 전역 prefix 가 소유한 것인가" 를 확인한다. 이
비교 대상이 패키지 스크립트였는데, unix 는 심링크가 그 스크립트로 정규화되어 일치하지만 Windows
에서 찾는 것은 `codex.cmd` 래퍼라 일치하지 않는다. 그래서 Codex 는 "네이티브 업데이터 없음" 으로
보고되고 UI 는 공식 안내 링크만 제공했다.

비교 시 Windows 래퍼를 그 래퍼가 실행하는 스크립트로 따라가도록 바꿨다. 수정 뒤 UI 가
"업데이트" 버튼을 제공하는 것까지 확인했고, 실제 업데이트 실행은 하지 않았다.

#### 8-5. Claude 모델 디스커버리가 Windows 에서 항상 실패 (`claude.rs`, `codex_executable.rs`)

Claude Code 를 npm 으로 설치한 뒤에도 에이전트 추가 창에 `provider model discovery failed` 가 떴다.
`claude auth status` 는 로그인 상태(exit 0)로 정상이었다.

디스커버리는 `claude` 경로를 Node 로 실행하는 SDK 브리지에 넘기고, 브리지가 그 경로로 Claude 를
띄워 모델 목록을 받는다. Windows 에서 PATH 탐색이 고르는 것은 npm 래퍼 `claude.cmd` 인데, Node 는
보안 수정 이후 `.cmd` 파일을 셸 없이 실행하지 않는다. 같은 SDK 호출을 직접 재현하면 `claude.cmd`
에서는 `spawn EINVAL` 이 나고, 래퍼가 실행하는 `claude.exe` 를 넘기면 모델 목록이 돌아왔다. 세션
시작과 사용량 조회도 같은 경로를 쓰므로 같은 이유로 실패했을 것으로 본다.

npm 래퍼 파서를 확장자별로 일반화해 `.exe` 대상도 따라가게 했고(`npm_cmd_shim_native`), Claude
디스커버리 · 사용량 조회가 래퍼 대신 그 실행 파일을 쓰도록 했다(`claude_executable`). 세션은
디스커버리가 기록한 경로를 쓰므로 함께 바뀐다. 래퍼 안의 대상은 `node_modules` 아래 경로만
따른다. JS 래퍼에 들어 있는 `"%dp0%\node.exe"` 는 Node 인터프리터라서, 이 조건이 없으면 `.exe` 탐색이
그것을 패키지로 오인한다. 수정 뒤 패키지 앱에서 오류 문구가 사라지고 모델 · 추론 강도 선택이
나타나는 것까지 확인했다. Claude 실제 턴은 실행하지 않았다.

모델 이름: SDK 의 `displayName` 은 메뉴용 별칭("Default (recommended)", "Opus", "Fable")이라 버전이
보이지 않았고, `default` 와 `sonnet` 이 같은 `claude-sonnet-5` 로 풀려 Sonnet 이 "Default" 로만 보였다.
브리지가 SDK 가 알려준 실제 모델 id 에서 이름을 만들도록 했다(`claude-fable-5-1` → `Fable 5.1`).
패키지 앱 드롭다운에 `Fable 5.1 / Opus 5 / Sonnet 5` 로 표시되는 것을 확인했다.

남은 점: Haiku 는 목록에 없다. SDK 가 Haiku 를 날짜가 붙은 `claude-haiku-4-5-20251001` 로 돌려주고
추론 강도도 주지 않아, 목록 검사와 실행 시 검증을 모두 통과하지 못한다. 기본 모델로 지정된
`claude-haiku-4-5` 가 목록에 없으므로 모델을 직접 골라야 한다. 목록은 SDK 가 주는 것만 쓰며,
SDK 에 없는 버전(Opus 4.6 등)은 실행 시 검증에서 막히므로 추가하지 않았다.

#### 8-6. Claude 턴이 Windows 에서 시작 2초 만에 실패 (`provider-runtime/claude-agent-sdk-bridge.mjs`)

Claude 에이전트가 메시지를 받으면 턴 시작 약 2초 뒤 `Claude Agent SDK returned an invalid protocol
receipt.` 로 끝나고 "복구 필요" 상태가 됐다. 브리지는 실패 원인을 버리고 이 문구만 남기므로, 브리지
사본에 검사 값을 출력하게 하고 사용자 승인 하에 Claude 턴을 한 번 실행해 확인했다.

세션 · 모델 · 권한 모드 · 도구 · fast mode 는 모두 일치했고 `cwd` 만 달랐다. 런타임은 작업 폴더를
canonicalize 한 verbatim 경로(`\\?\C:\...\temp agents`)로 넘기는데, Claude Code 는 init 메시지에 같은 폴더를
Win32 형식(`C:\...\temp agents`)으로 보고한다. 브리지는 둘이 정확히 같아야 턴을 진행한다.

비교할 때 양쪽에서 verbatim 접두사(`\\?\`, `\\?\UNC\`)만 떼도록 했다. 드라이브 · 공유 · 폴더가 다르면 여전히
실패한다. 수정 뒤 같은 에이전트를 중지 → 재개하자 보류돼 있던 메시지에 정상 응답하고 대기 상태로
돌아가는 것을 패키지 앱에서 확인했다.

#### 8-7. Node 없음을 "Claude CLI 없음" 으로 보고 (리뷰 지적 3)

Claude 카탈로그 조회는 Claude 실행 파일과 별개로 `node` 를 찾아 SDK 브리지를 띄운다. 둘 다 `Missing`
이면 같은 `command_missing` 이 되므로, Claude 가 설치·로그인되어 있어도 Node 가 없으면 "Claude Code
CLI 없음" 으로 보이고 설치를 눌러도 `AlreadyInstalled` 만 반복된다. 브리지용 Node 부재를 별도 실패
(`bridge_runtime_missing`)로 구분하고, 화면에는 Node 설치·경로 안내를 보여준다. 이 PC 에는 Node 가
있어서 실제로 재현한 상태는 아니고, 코드 경로와 분기만 확인했다.

#### 8-8. npm 프로젝트 로컬 래퍼 지원 범위 (리뷰 지적 4)

래퍼 파서는 대상 경로의 첫 구성 요소가 `node_modules` 여야 했다. npm 의 프로젝트 로컬 실행기는
`node_modules/.bin` 에 있고 대상이 `..` 로 시작하므로 거부됐다. 래퍼가 있는 디렉터리가 실제로
`node_modules/.bin` 일 때만 앞의 `..` 하나를 허용하도록 했다. 두 번째 `..`, `.`, 빈 구성 요소는 그대로
거부하며, 동반 파일 검증도 그대로다. 전역 설치 환경에서만 실행으로 확인했고, 프로젝트 로컬 설치는
단위 테스트로만 확인했다.

### 9. 없는 provider CLI 를 앱에서 설치

`configured command missing` 은 상태 표시일 뿐 해결 수단이 없었다. 이제 확인 절차를 거쳐 앱이 직접
설치한다.

- `POST /api/providers/install/check` 가 설치 제안을 만든다. 런처가 실제로 없어야 하고, npm 이
  있어야 하며, 버전은 npm 레지스트리의 `latest` 다. 응답에는 실행할 인자 목록이 그대로 들어간다.
- UI 는 그 명령을 그대로 보여주고, 사용자가 "설치" 를 누른 뒤에만
  `POST /api/providers/install/start` 로 **확인된 그 버전**을 설치한다. 버전이 달라졌으면 거부한다.
- 설치 후 런처를 다시 탐색해 npm 전역 prefix 안에 있는지 확인하고, 카탈로그를 갱신한다. PATH 에
  prefix 가 없으면 성공으로 넘기지 않고 `provider_install_outside_path` 로 보고한다.
- 대상은 공식 npm 패키지가 있는 Codex(`@openai/codex`), Claude Code(`@anthropic-ai/claude-code`),
  OpenCode(`opencode-ai`) 뿐이다. 카탈로그의 `install_supported` 가 이를 알려주므로 UI 는 설치할 수
  없는 provider 에 버튼을 띄우지 않는다.
- 전역 설치는 하나의 prefix 를 공유하므로 한 번에 하나만 실행한다. 시작된 설치는 요청 핸들러가
  사라져도 작업이 끝까지 소유한다.

리뷰 후 보완(리뷰 지적 1 · 2 · 5):

- 설치도 업데이트와 같은 결과 소유 구조를 쓴다. 서비스가 설치 작업과 결과를 보관하므로, 요청이
  사라져도 다른 요청이 같은 작업에 합류하고, `CleanupUnconfirmed` 는 다음 설치 요청과 종료까지
  유지된다. `shutdown()` 은 업데이트뿐 아니라 설치 결과도 합류해 확인한다.
- 확인 화면에서 본 제안 전체(provider · 패키지 · 버전 · 실행 명령, 명령에는 npm prefix 가 들어 있다)를
  시작 요청이 그대로 돌려보낸다. 서버는 계획을 다시 만들어 그 제안과 정확히 같을 때만 실행하고,
  prefix 나 버전이 바뀌었으면 `OfferChanged` 로 거부해 다시 확인을 받게 한다. 이전에는 버전만 비교했다.
- 복사 버튼과 화면 표시는 공백이 있는 경로를 따옴표로 감싼다(`shellCommandText`). 앱 내부 설치는
  인자 배열을 그대로 쓰므로 이 변경과 무관하다.

`AGENTS.md` 는 새 설치 경로를 소유자 승인 사항으로 둔다. 이 기능은 소유자 결정으로 추가했고,
"창을 여는 것은 설치 프로그램을 시작하지 않는다" 는 기존 딥링크 계약은 그대로다. 앱이 실행하는
것은 사용자가 화면에서 읽고 확인한 고정 명령뿐이다.

또한 없음 상태 문구를 바꿨다. `configured command missing` 원문 대신 "이 PC에서 <이름> CLI를 찾지
못했어요. 데스크톱 앱에만 포함된 CLI는 다른 앱에서 사용할 수 없어요." 로 안내한다.

### 10. 설치 · 업데이트 카드 UI와 업데이트 뒤 남는 카드 (`frontend`)

처음 판은 방 초대 화면의 상태 블록과 확인 다이얼로그를 재사용했다. 실제로 써 보니 두 가지 문제가
있었다.

- CLI 업데이트가 끝난 뒤에도 "업데이트 중" 배지와 카드가 계속 남았다. 원인은 업데이트 시작 시
  켜는 플래그(`installing`)를 끄는 곳이 없었던 것과, 완료 상태가 사라지는 조건 없이 계속
  렌더링되던 것으로 봤다.
- 초대 화면용 블록이라 이 용도에 맞지 않았다(provider 구분 없음, 버전 비교 없음, 설치 명령 확인이
  별도 다이얼로그).

첫 개편판(그라디언트 카드, 점 배지, 로고 타일)은 바로 위 선택된 provider 칩과 로고 · 이름을
반복했고 앱의 다른 UI와 따로 놀았다. 두 번째 판에서는 provider 그리드 바로 아래에 붙는 한 줄
행으로 바꿨다(`ProviderSetupCard`, `styles/provider-setup.css`).

- 그리드 칩과 같은 배경 · 모서리 · 글자 크기를 쓰고, 상태는 왼쪽 아이콘 색으로만 구분한다
  (설치 필요 주황 · 새 버전 파랑 · 완료 초록 · 실패 빨강). provider 이름과 로고는 반복하지 않는다.
- 버전은 `2.1.274 → 2.1.276` 처럼 이전 버전에 취소선을 긋고, 새 버전에서 바뀐 자리만 강조한다.
- 설치 확인은 별도 다이얼로그 대신 행 아래에 `$` 가 붙은 명령 한 줄과 복사 버튼을 펼친다. 설치가
  시작되면 같은 줄에 커서가 깜박이고, 행 오른쪽에 경과 시간이 표시된다.
- 업데이트 프롬프트는 진행 중인 요청이 "확인"인지 "업데이트"인지를 상태로 들고, 요청이 끝나면
  `finally` 에서 비운다. "업데이트하는 중" 표시는 이 상태에서만 나온다.
- 설치 · 업데이트 완료 행은 약 2.6초 보여준 뒤 높이를 접으며 사라진다(`useTransientResult`).
- 버전 확인 중에는 행을 계속 보인다. 확인 요청이 다른 화면이 시작한 업데이트에 합류할 수 있고,
  그동안 에이전트 생성이 막히기 때문이다(`AgentCreateModal.update` 테스트가 이를 요구한다).

첫 개편판은 패키지 앱에서 31px 로 눌려 보였다. 에이전트 대화상자 본문이 스크롤되는 flex column
이라 카드가 줄어들면서 `overflow: hidden` 에 잘린 것으로 판단했고, `flex: none` 을 줬다.

새 CSS 규칙이 추가되므로 CSS 승인 게이트(`frontend/scripts/verify-original-css.mjs`)의 승인
엔트리를 `index-DB2ORLYr.css` 로 갱신했다. 원래 승인본과 비교해 추가된 54개 규칙은 모두 `dc-setup`
계열이고 기존 규칙은 하나도 바뀌거나 빠지지 않았다. Linux 컨테이너(node:24-bookworm)와 Windows 빌드에서 같은 SHA-256 이 나오는 것을
확인한 뒤 갱신했다. 인라인 스타일 문자열 `"break-all"` 이 Tailwind 에 유틸리티로 잡혀 게이트에
걸린 일이 있어 `overflowWrap: "anywhere"` 로 바꿨다.

### 11. 런타임 내장 파일 도구가 Windows 에서 모든 쓰기를 거부 (`crates/agentsassemble-provider/src/workspace_files.rs`)

API provider(DeepSeek)에게 작업 폴더 쓰기를 요청하자 `write_workspace_file` 이 매번
`workspace_tool_failed` 를 돌려줬다. 읽기와 목록은 정상이었다.

원인은 경로 표기로 봤다. `resolve()` 와 `discover()` 가 `PathBuf::to_str()` 로 계약 경로를 만드는데,
Windows 에서는 구분자가 `\` 가 된다. 도구 계약의 `relative()` 는 `\` 와 `:` 를 거부하므로, 내부에서
다시 검증하는 쓰기 · 치환 경로가 전부 실패했다. 읽기는 이 재검증을 거치지 않아 드러나지 않았다.

`contract_path()` 를 추가해 Windows 에서는 경로 구성 요소를 `/` 로 이어 붙이게 했다. `.` 는
건너뛰고, 일반 이름이 아닌 구성 요소(루트 · 드라이브 · `..`)가 나오면 원래 표기를 그대로 넘겨 기존
거부 로직이 처리하게 했다. 다른 플랫폼은 동작이 같다.

### 12. CSS 승인 게이트를 제거 (`frontend/scripts/verify-original-css.mjs`)

빌드된 CSS 번들 전체의 SHA-256 과 청크 파일명을 스크립트에 박아 두고, 값이 다르면 `npm run build`
를 실패시키는 게이트였다. 원본 디자인의 폭포가 실수로 바뀌는 것을 막는다는 목적이었다.

기록을 보면 이 게이트가 회귀를 잡은 적은 없다. 승인 값을 건드린 커밋 4건이 전부 값을 새로 승인한
것이고, `docs/VERIFICATION.md` 에도 "의도한 폭포 변경마다 게이트를 갱신한다"고 적혀 있다. 즉 통과
문장을 만들어 내는 절차로 굳었고, 실제로 걸린 세 번은 모두 기존 규칙을 건드리지 않은 경우였다.
Tailwind 가 소스 문자열을 훑기 때문에 인라인 스타일 값 `"break-all"`, `inline-grid`, 심지어 주석에
쓴 "invisible" 이라는 단어가 규칙을 새로 만들어 낸 것이었다. 실패 메시지는 해시 두 개뿐이어서 무엇이
달라졌는지 매번 두 빌드를 비교해 찾아야 했다.

전체 바이트 해시는 "기존 규칙이 바뀌거나 사라졌다"(막아야 하는 것)와 "규칙이 추가됐다"(무해한 것)를
구분하지 못한다. 규칙 단위로 비교하도록 다시 쓰는 방안을 검토했지만, 이 게이트가 지키려던 기준선이
지금은 원본 디자인이 아니라 직전 빌드 출력이고 그 출력 자체를 고쳐야 하는 상황이라 유지할 이유가
없다고 판단해 스크립트와 `build` 스크립트의 호출을 지웠다. CI(`windows-contract.yml`)는 프론트엔드
빌드를 돌리지 않으므로 CI 에서 빠지는 검사는 없다.

디자인이 의도 없이 바뀌는 것은 소스 diff 와 실제 화면 확인으로 본다. 빌드 산출물 해시는 소스
diff 가 이미 보여 주는 것에 더해 주는 정보가 없었다.

### 13. 클로드가 "복구 필요" 로 잠기는 경로 (`room_portal` · `runtime_turn.rs`)

방에서 클로드에게 "못 쓰는 툴을 호출해보라" 고 하자 세션이 **복구 필요** 로 잠겼다. 재현은
`roll_dice` 한 줄로 충분했다. 이 방은 `tool_mode` 가 일반 대화라 주사위 도구가 없다.

원인은 턴 계약을 어긴 것이었다. 에이전트에게 주는 지시문은
`room_turn_context.rs:378` 한 문장이다.

> Call `read_discussion` before deciding. Then finish with exactly one terminal action
> exposed by the room transport.

여기서 "terminal action" 이 무엇인지 열거하지 않는다. 실제로 턴을 닫는 것은 `publish_message`,
`decline_to_speak`, 투표 명령 세 가지뿐이고 (`room_portal_mcp.rs` 에서 `active.outcome` 을
등록하는 곳), `roll_dice` 나 `read_discussion` 은 그냥 도구다. 클로드는 자기 턴에 대해 이렇게
설명했다 — "roll_dice 호출 자체를 그 턴의 terminal action 으로 간주했고, 이미 완결된 action
이었기 때문에 publish_message 로 별도 보고를 추가하지 않고 턴을 마쳤다."

도구 거부 자체는 정상 동작이었다. `room_portal.rs:514` 가 문자열 오류를 돌려주고 턴은 살아
있으며, 클로드도 거부당한 사실을 알고 있었다. 빠진 것은 그 뒤의 발행이다.

방에 아무것도 등록되지 않은 채 턴이 끝나면 `finish_room_observation` 이
`room_portal_publication_missing` 을 돌려주고, 호출부가 그것을
`ProviderAdapterError::uncertain(...)` 으로 감싼다 (`runtime_turn.rs:442`). `uncertain()` 은
`effect_uncertain: true` 를 하드코딩하므로 (`runtime.rs:107`) 드라이버 설정과 무관하게 격리
대상이 된다. 서버는 `effect_uncertain && !runtime_stopped` 를 보고 턴을 격리한다
(`provider_turn.rs:314`).

이 경로는 클로드 전용이 아니다. `finish_room_observation` 은 claude · codex · cursor_acp ·
grok_acp · opencode · remote_openai · managed_bridge_parent 가 모두 구현하고 같은
`portal.finish_turn(request)` 을 부른다. 다만 API 드라이버는 앱이 도구 루프를 직접 돌려 종결
동작까지 밀어붙일 수 있고, 외부 CLI 계열은 에이전트가 스스로 멈추면 붙잡을 방법이 없다.

방의 에이전트 넷(Terra · deepseek-flash · Sonnet 5 · grok-4.6)에게 이름을 물었을 때 셋이
`pass_turn`, grok 은 `skip_turn` 을 골랐다. grok 의 근거는 이 방에서 "턴 넘기기" 가 이미 발행 +
다음 에이전트 지정을 뜻한다는 것이었고, deepseek 과 Sonnet 은 `reason_code` 를 유지한다면
`abstain` 이 스키마에 더 맞다고 덧붙였다. grok 이 말한 겹침은 에이전트들이 쓰는 한국어 표현에서
생기는 것이지 도구 스키마끼리는 구분되며, 화면이 이미 "차례 넘김" 이라고 부르고 있어 `pass_turn`
으로 정했다.

그래서 세 가지를 바꿨다.

- **도구 이름** — `decline_to_speak` 를 `pass_turn` 으로 바꿨다. 설명도 "할 말이 없을 때 턴을
  끝내는 보통의 방법" 이라고 적었다. 사유 코드(`nothing_useful_to_add` 등)는 그대로다.
- **지시문과 도구 설명** — 지시문이 턴을 끝내는 동작을 `publish_message`, `pass_turn`, 투표
  명령으로 명시하고, 읽기 · 검색 · 첨부 · 무작위 도구는 여러 번 써도 되며 턴을 끝내지 않는다고
  적는다. 실패한 도구 뒤에도 발행이나 넘김이 필요하다는 것도 적었다. 읽기 · 무작위 도구 설명에는
  "Does not end the turn" 을 붙였다. 이번 사고가 정확히 이 구분을 모델이 놓친 것이라서다.
- **안전망** — `read_discussion` 은 했는데 아무것도 등록하지 않은 채 턴이 끝나면, 포털이 오류
  대신 사유 `no_publication` 인 넘김을 돌려준다. 방에 보이는 효과가 없으니 불확실한 것이 아니고,
  잠그지 않고 차례를 넘긴다. 화면의 차례 넘김 줄에는 "발언 없음" 으로 뜬다. `no_publication` 은
  에이전트가 고를 수 있는 사유가 아니라 `valid_decline_reason` 에는 넣지 않고, 영속화 쪽 허용
  목록에만 더했다. 방을 읽지도 않고 끝난 턴(`room_observation_unconfirmed`)과 형식이 잘못된
  종결 동작은 그대로 오류로 둔다.

이로써 `OutcomeMissing` 은 만들어지는 곳이 없어져 지웠다. 이 동작을 고정하던 테스트 셋
(`room_portal_mcp`, `opencode`, `remote_openai` 포털 테스트)은 새 동작에 맞게 고쳤고, 지시문
문구를 고정하던 `ordered_assignment_and_finalization_are_durable_and_exact` 는 종결 동작이
열거되는지를 보도록 바꿨다.

API 드라이버의 `room_tool_rejected` 사유가 버려지는 문제(세션 초반 기록)는 이번에 손대지 않았다.

### 14. 격리된 턴이 아무 근거도 남기지 않음 (`claude_sdk_runtime.rs` · `provider_turn_execution.rs`)

13 을 진단하려 했을 때 남아 있는 기록이 없었다. 세션의 `last_error` 는 "The exact provider turn
remains quarantined pending recovery." 한 문장뿐이고, `runtime.stderr.log` 는 0 바이트였다.

두 군데서 지워지고 있었다. 브리지의 stderr 는 파이프가 막히지 않도록 읽기만 하고 버려졌고
(`drain_stderr`), 격리 시 `last_error` 와 `last_error_code` 가 복구 문구로 덮어써졌다
(`provider_turn_execution.rs:267`).

stderr 는 마지막 8 KiB 를 보관해 실패한 턴의 오류 메시지에 붙이고,
`mark_provider_turn_recovery_required` 는 원인을 인자로 받아 복구 문구 뒤에 남기게 했다. 복구
동작(phase · 오류 코드 · 중지 계획)은 바꾸지 않았다. 고친 뒤 재현하자 이렇게 남았다.

```
The exact provider turn remains quarantined pending recovery.
Cause: room_portal_publication_missing: The provider did not stage a valid room publication or decline.
```

### 15. 재개한 세션이 며칠 전 메시지에 답함 (`agent_lifecycle.rs`)

에이전트를 재개하자 사람이 아무 말도 하지 않았는데 턴이 시작됐고, 넷이 3 분 동안 서로 차례를
넘기며 대화했다. `turn_started` 의 `source_event_id` 를 따라가니 **이틀 전** 이벤트였다.

중지된 세션에는 새 메시지가 쌓이지 않는다. `route_session_is_eligible` 이 `Attached` 이고
`provider_session_active` 인 세션만 대상으로 삼는다. 문제는 **중지 전에 답하지 못한 큐가 상한
없이 보존**된다는 점이었다. 중지 경로가 `merged_turn_queue` 로 큐를 다시 만들어 넘기기 때문에,
며칠 뒤 재개해도 그대로 남아 있었다.

처음에는 중지할 때 큐를 비우려 했으나 테스트 두 개가 거부했다.
`shutdown_never_assigns_work_to_an_already_stopped_runtime_in_either_order` 와
`failed_turn_stop_uses_confirmed_exit_but_never_skips_retained_runtime` 이 각각 "앱을 종료해도
답하지 못한 입력은 남아야 한다" 를 고정하고 있었다. 타당한 요구라 큐를 비우는 대신 나이 제한을
두었다. `merged_turn_queue` 가 이미 각 이벤트를 불러 삭제 여부를 보고 있어, 같은 자리에서 한
시간이 지난 입력을 버린다. 한 시간이라는 값에 근거는 없고, 짧은 재시작은 이어가고 하루 지난
것은 버린다는 선만 그은 것이다.

### 16. 자유 토론 모드에서 관찰된 실패 두 건 (원인 미확인)

사용자가 방을 자유 토론(`conversation_mode: ambient`)으로 바꾸고 "가위바위보" 를 보내자 네
에이전트가 동시에 턴을 받았다. deepseek 은 답했고, 두 세션이 실패해 복구 필요로 남았다.

- **Sonnet 5** — 턴 시작 1 초 만에 `provider_protocol_invalid`. 14 에서 보존하게 된 stderr 꼬리에는
  SDK 기동 경고 한 줄(`CLAUDE_SDK_CAN_USE_TOOL_SHADOWED`: 방 도구가 `allowedTools` 로 자동 승인돼
  `canUseTool` 이 불리지 않는다는 내용)만 있었다. 이 경고는 기동할 때마다 찍히므로 이번 실패의
  원인이라고 볼 근거는 없다. 브리지는 치명 오류를 stderr 가 아니라 stdout 프로토콜
  (`claude_sdk_protocol_failed`)로 알리기 때문에, 이런 경우 stderr 꼬리는 원인을 담지 못한다.
- **grok-4.6** — 약 56 초 뒤 `provider_turn_failed` ("The ACP provider request could not
  complete.").

같은 시점에 Terra(Codex)도 턴을 받은 채 멈춰, 약 1 시간 반 뒤 앱을 재시작할 때에야 실패로
정리됐다.

이후 재현을 시도했다. 자유 토론 모드에서 Sonnet 5 하나만 켜고 같은 메시지를 보내자 5 초 만에
정상 답했다. 넷을 모두 켜고 한 메시지를 보내자 수십 턴이 동시에 돌았고 잠긴 세션은 없었다.
그래서 자유 토론 모드 자체나 동시 실행이 원인이라는 근거는 없다. 실패한 두 세션(Sonnet 5 ·
Terra)은 그 턴 직전 약 1 시간 46 분 동안 쉬고 있었다는 공통점이 있지만, 그만큼 기다리는 재현은
하지 않았으므로 추정이다.

재현하면서 14 의 보존 방식에 결함이 있는 것을 찾았다. 영속화되는 진단 문구는 끝에서 512 자만
남는데, 오류 문구 뒤에 stderr 를 붙였기 때문에 긴 stderr 가 정작 오류 문구를 밀어내고 마지막
"." 만 남았다. Sonnet 5 의 기록이 ". [bridge stderr] …" 로 시작한 이유다. 이제 stderr 를 앞에 두고
길이를 제한해 오류 문구가 끝에 남게 했다. 또 격리 시 남기는 원인이 `redact_persisted_diagnostic_text`
를 거치지 않아, stderr 에 든 로컬 경로나 토큰이 그대로 저장될 수 있었다. 다른 진단 문구와 같은
가림 처리를 거치게 했고, 경로가 `[local path]` 로 바뀌는지 테스트에 넣었다.

자유 토론 모드에서는 새 메시지마다 모든 에이전트가 다시 턴을 받으므로, 한 에이전트가 "넘김" 을
여러 번 하는 것은 각각 다른 메시지에 대한 판단이다. 재현 중 Sonnet 5 의 넘김 14 번이 모두 서로
다른 메시지(다른 에이전트가 올린 것)에 대한 것이었다. 동작은 설계대로지만, 넘김 사이에 메시지가
끼어 차례 넘김 줄이 합쳐지지 않고 턴마다 비용이 든다.

위 SDK 경고는 방 도구의 권한 요청이 `canUseTool` 을 거치지 않는다는 뜻이라, 별도로 확인할
가치가 있다.

### 17. 하네스 턴의 토큰 낭비 두 가지 (`room_portal_render.rs` · `acp_client.rs`)

**측정.** 2026-09-21 05:53–06:11 UTC(자유 토론 모드 가위바위보, 네 에이전트 동시)의 모델 호출을
각 프로그램이 남긴 사용량 기록으로 셌다. 출처: Claude 는
`%USERPROFILE%/.claude/projects/C--Users-Fleurdelys-Downloads-aa-temp-agents/*.jsonl` 의
assistant `usage`, grok 은 `provider-state/grok/<id>/logs/unified.jsonl` 의
`shell.turn.inference_done`, Codex 는 `%USERPROFILE%/.codex/sessions/2026/09/21/rollout-*.jsonl`
의 `token_count`. 차례 수는 같은 구간의 `provider_turn_executions` 로 셌다.

| 하네스 | 차례 | 차례당 모델 호출 | 호출당 입력(중앙값) | 차례당 입력 |
|---|---|---|---|---|
| Codex (Terra) | 14 | 3.1 | 20,520 | 약 6.3만 |
| Claude (Sonnet 5) | 20 | 3.0 | 46,730 | 약 14만 |
| grok-4.6 | 8 | 3.9 | 63,363 | 약 24만 |

입력의 92~99.7% 가 캐시였다. Codex 가 작은 것은 Terra 를 그날 만들어 기록이 짧았기 때문이라
공정한 비교가 아니다. Claude 는 9월 18일부터 같은 세션을 이어 써 기록이 565 항목이었다. 기록이
쌓여 매번 다시 보내지는 것은 모든 에이전트가 그렇게 동작하는 정상이며, 커서 설계상 일부러 세션을
이어 쓰는 것이다(새 내용만 주고 옛 내용은 에이전트 쪽 기록에 맡긴다).

grok 은 방에서 쓰지 않는 자체 툴 28 개의 설명(약 6만 자)을 매 호출 싣고, 방 툴이 `use_tool` 우회
툴 뒤에 있어 툴 검색(`search_tool`) 비용도 든다. grok CLI 에는 `--tools` / `--disallowed-tools` 가
있으나 우리는 쓰지 않는다(Claude 는 읽기 전용이면 `tools: ["AskUserQuestion"]` 로 제한한다). 이것은
고치지 않았다. 같은 "방 읽기 전용" 에서 grok 은 허락이 필요 없는 자체 툴(이미지 생성, 웹 가져오기,
파일 읽기 등)을 쓸 수 있을 것으로 보이며(실사용 확인은 안 함), 명령 실행은 권한 거절로 막혔다
(기록상 5 회 시도 모두 거절).

**고친 것 1 — 방 읽기 결과가 너무 컸다.** 검색 결과마다 ID 두 개·긴 타임스탬프·메시지 본문
전체가 들어갔고, 문맥 읽기는 `RoomEvent` 를 통째로 돌려줘 한 번에 15,908 자가 나온 적이 있다.
이 결과는 하네스 기록에 남아 이후 모든 호출에 다시 실린다. 이제 `#seq 작성자: 본문` 한 줄씩으로
보내고, 검색은 300 자 미리보기, 문맥은 메시지당 1,200 자로 자른다. 후속 읽기에 필요한 event ID
는 남긴다. 모든 드라이버가 같은 포털 MCP 를 쓰므로 전부에 적용된다. 이 때문에 12,000 자 메시지
넷을 검색하면 예전처럼 `room_tool_result_too_large` 로 실패하지 않고 미리보기로 성공한다(크기 상한
자체는 첨부 테스트가 여전히 검증한다).

**고친 것 2 — 종결 툴 뒤의 마지막 호출 (ACP 만).** 하네스는 툴 결과를 받으면 모델을 한 번 더
부른다. 발행·넘김 뒤 그 호출로 쓰는 마무리 문장은 방에 올라가지 않는다(방 턴은 포털 결과로
대체된다). grok 기준 그 호출의 입력이 약 6.8만 토큰(대부분 캐시)이었다. API 드라이버
(`remote_openai`)는 원래 종결 툴에서 바로 끝냈다. 포털이 종결 동작 등록을 알리게 했고
(`TerminalWatch`), ACP 클라이언트(grok · Cursor)는 그 신호에서 `session/cancel` 을 보내고 취소 또는
정상 종료를 정상 턴 종료로 처리한다. 실제 grok 턴 **한 번**에서 읽기 → 발행 → 취소로 끝나 세 번째
호출이 없었고, 메시지는 바로 올라갔으며 세션은 대기로 돌아왔다. 같은 세션의 **두 번째 턴은 돌려보지
않았다.**

**안 한 것.** Claude 와 Codex 는 아직 마지막 호출이 남아 있다. Codex 는 `turn/interrupt` 로 구현해
단위 테스트까지 통과시켰으나, 가짜 app-server 경계 테스트(`agent_session_boundary`)와 실제 턴을
돌리지 못해 커밋하지 않고 되돌렸다. 알림을 읽는 도중에 끊으면 스트림이 깨지므로, 알림 사이에서
`TerminalWatch` 의 등록 여부를 확인하는 방식이어야 한다. Claude 는 종결 툴 뒤에 멈추게 하는 훅이
필요하고, 그 결과 메시지가 브리지의 `validResult`(subtype · terminal_reason 등)를 통과하는지 실제
턴으로 확인해야 한다.

**열린 문제 — grok 이 켜진 상태의 앱 종료 (첫 종료 시도 뒤 16:22:49 에야 꺼짐, 정상 아님).** 경과:

- 16:01:23 앱 시작. 16:02 조기 취소 grok 턴 1 회(위). grok 로그에는 `cancel.received` ·
  `cancel.processing` 만 있고, 평소 턴 끝에 찍히는 `handle_prompt.done` 이 없었다. 그 뒤 grok 로그는
  16:06:59 인증 갱신 한 줄뿐이었다.
- 첫 종료 시도는 PowerShell `CloseMainWindow()` 였는데 창이 닫히지 않았다. 그 핸들은 제목이 빈
  보조 창이었고(실제 창 제목은 "AgentsAssemble"), 도구 쪽 문제였다. 이어서 `taskkill /PID <desktop>`
  (강제 아님)로 창을 닫았다. 창이 닫힌 정확한 시각은 기록하지 않았다.
- 창이 닫힌 뒤에도 `/healthz` 가 계속 200 을 돌려줬고, `runtime.sqlite3` 의 마지막 수정 시각은
  16:03:46 에서 멈춰 있었으며, 종료 뒤 DB 에서 grok-4.6 세션은 `attached / idle` 로 남아 있었다.
  서버가 종료 정리(세션 중지 기록)를 시작하지 않았다는 뜻으로 본다. 오래 정리한 것이 아니라 종료가
  서버까지 전달되지 않은 것이다.
- 16:22:49 에 모든 프로세스가 사라졌다. 사용자가 그 무렵 터미널을 닫았다고 했고, 데스크톱의
  최종 강제 종료 한도(`DESKTOP_SHUTDOWN_GRACE` 683 초 = 서버 680 초 + 3 초)에 걸린 것과 구분하지
  못했다. 어느 쪽이든 정상 종료는 아니다.

조기 취소와 이 종료 문제의 관계는 확인하지 못했다. 서버가 종료 자체를 시작하지 않은 점을 보면 grok
정리를 기다린 것은 아닌 것으로 보이나 배제하지 못했다. 이 문서 앞부분에 적힌 "데스크톱이 정상 종료
시 가끔 안 꺼진다" 증상과 같은 것일 수 있다. 다음 확인: (1) 같은 grok 세션으로 두 번째 턴이
정상인지, (2) 에이전트를 모두 중지한 뒤 닫을 때와 grok 을 켠 채 닫을 때의 종료 시간, (3) 조기 취소
이전 커밋(`61cf466`)으로 같은 순서를 반복. 다음 기동 때 남은 grok 세션이 어떤 상태로 정리되는지도
볼 것. 그전까지는 앱을 닫기 전에 에이전트를 중지해 두고, 몇 초 안에 안 꺼지면 기다리지 말고 원인을
볼 것.

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
- 실제 Tauri 빌드에서 CSS 승인 게이트 통과, Linux 컨테이너 빌드와 해시 일치 (게이트는 이후 12 에서 제거)
- 프론트엔드 전체 테스트 899개, `human_invite_manager_boundary` 7개, 첨부 저장 4개
- 패키지 앱에서 외부 접속이 꺼진 상태로 "이 PC의 AI 초대 만들기" 버튼 활성
- Grok 의 커넥터 MCP 기동과 도구 노출
- 패키지 앱에서 Codex 가 모델 목록 · 버전 확인 · "업데이트" 버튼까지 표시 (8-2, 8-4)
- 패키지 앱에서 Grok 기존 세션 시작: 자식 프로세스 유지, 복구 문구 없이 대기 진입 (8-1).
  앱 종료 시 자식까지 정리됨
- 패키지 앱에서 Claude Code · OpenCode in-app 설치: 확인 창의 명령 그대로 실행,
  각각 6초 · 15초에 완료, 재시작 없이 카탈로그 갱신. 디스크에서 `claude.cmd --version` 확인
- 설치 불가 provider(Cursor)에서 설치 버튼 없이 안내만 표시
- 패키지 앱에서 Grok 에이전트 실제 턴: 작업 폴더 `C:\Users\Fleurdelys\Downloads\aa\temp agents`
  (이름에 공백), 권한 "작업 폴더 쓰기". 파일 작성을 요청하자 에이전트 메시지 안에 권한 요청
  (`Write hello-from-grok.txt`)이 떴고, "한 번 허용" 으로 응답하자 파일이 디스크에 정확한 내용으로
  생성됐다. 에이전트는 파일을 다시 읽어 내용을 방에 답했고 대기 상태로 돌아왔다
- 패키지 앱에서 DeepSeek API 에이전트(`deepseek-flash`, 작업 폴더 쓰기) 실제 턴. 사용자가 앱에
  API 키를 등록한 뒤 실행했다. 11 을 고치기 전에는 쓰기가 `workspace_tool_failed` 로 실패했고,
  고친 뒤 같은 요청에서 방에 권한 요청(`hello-from-deepseek.txt 파일을 생성하거나 덮어씁니다`)이
  떴다. "이번 변경 허용" 으로 응답하자 파일이 디스크에 정확한 내용(43바이트)으로 생성됐고,
  에이전트가 다시 읽어 같은 내용을 답했다
- 설치 · 업데이트 행의 각 상태를 실제 컴포넌트와 빌드된 CSS 로 렌더링해 확인. 패키지 앱에서는
  Claude Code 의 실제 업데이트 제안(2.1.274 → 2.1.276)이 그리드 아래 44px 행으로 표시되는 것까지
  봤다. 업데이트는 실행하지 않았으므로, 완료 후 행이 접히는 장면은 단위 테스트(3.2초 뒤 영역
  없음)로만 확인했다
- 클로드 세션이 `roll_dice` 를 부르고 발행 없이 턴을 끝내 격리되는 것을 재현했고, 14 를 고친 뒤
  같은 재현에서 `Cause: room_portal_publication_missing: …` 이 세션에 남는 것을 확인했다.
  클로드 실제 턴 실행은 사용자가 이 목적으로 승인했다
- 방의 에이전트 넷(Terra · deepseek-flash · Sonnet 5 · grok-4.6)에게 실제 턴으로 이름 후보를
  물었다. 넷 다 한 번씩만 답했고 두 번째 차례는 `duplicate` 로 넘겼다. 이때 보낸 질문 본문이
  cp949 로 기록돼 에이전트에게 깨진 채 도착했는데, 이는 질문을 만든 쪽 도구의 문제이고 앱의
  인코딩은 정상이었다(같은 턴의 `room_view` 는 올바른 UTF-8). 두 에이전트가 이 사실을 정확히
  신고했다
- 13 의 세 가지 변경 후 provider 테스트 183 통과(기존 실패 3 은 그대로), persistence 334
  통과(기존 실패 5 는 그대로), 프론트엔드 914 통과. 발행 없이 끝난 턴이 `no_publication` 넘김이
  되는 것은 실제 MCP 포털을 쓰는 `remote_openai` 포털 테스트로 확인했다

실행하지 않은 것:

- `managed_bridge::…managed_api_uses_private_credentials…` 테스트는 여전히 실패한다. 같은 관리
  경로로 DeepSeek 가 실제로 동작했으므로 테스트 환경 쪽 문제일 가능성이 있지만 원인은 확인하지
  않았다
- Codex · OpenCode 의 실제 턴
- 13 의 변경을 패키지 앱에서 실제 턴으로 재현하는 것. 확인 시점에 방이 사용자가 바꾼 자유 토론
  모드였고 클로드가 16 의 실패로 복구 필요 상태여서, 설정을 되돌리지 않고 테스트로만 확인했다
- Codex 실제 업데이트 실행. 버튼이 제공되는 것까지만 확인했다
- Claude 로그인. 계정 로그인은 사용자 몫이다
- 서버 · persistence 크레이트 전체 테스트
- 공개 인그레스(cloudflared) 전체 경로
- 롤링 재시작. `runtime_reexec::InheritedListeners` 는 `cfg(unix)` 전용이다
- 다른 설치 방식(pnpm · yarn · bun · winget)이나 다른 계정 환경

## Windows 테스트 현황

`cargo test -p agentsassemble-provider --lib` 를 이 PC 에서 샌드박스 밖으로 실행했다. 변경 전
커밋(`4a319f9`)과 이 브랜치를 같은 조건으로 비교했다.

| | 통과 | 실패 |
| --- | --- | --- |
| `4a319f9` | 166 | 4 |
| 이 브랜치 (11 수정 전) | 171 | 4 |
| 이 브랜치 (11 수정 후) | 173 | 3 |
| 이 브랜치 (8-5 수정 후) | 174 | 3 |

처음 늘어난 5개는 이 브랜치가 추가한 테스트(Codex 래퍼 2, Grok 홈 1, 설치 2)다. 8-5 에서 래퍼 파서
테스트 1개가 더해졌다. 11 을 고친 뒤
`workspace_tools::tests::file_tools_preserve_boundaries_and_exact_replacement` 가 통과했고,
두 커밋 모두에서 90초 이상 끝나지 않던
`workspace_tools::tests::writes_require_exact_owner_response_and_delivery_receipt` 도 끝까지
실행되어 통과했다. 두 테스트 모두 11 과 같은 경로 표기 문제였던 것으로 본다.

남은 실패 3건은 변경 전 커밋에서도 실패하던 테스트다.

- `selection::tests::workspace_path_is_exact_and_per_model_relations_are_mandatory` —
  `" workspace "` 를 기대하지만 Windows 가 이름 끝 공백을 제거해 `" workspace"` 가 된다
- `managed_bridge::windows_tests::managed_api_uses_private_credentials_and_confirms_whole_job_stop`,
  `managed_bridge::platform::native_tests::managed_native_stop_and_pipe_loss_remove_the_entire_nested_job` —
  `managed_bridge_protocol_failed`. Windows CI 가 실행하는 필터와 겹치므로 CI 환경과 이 PC 의
  차이는 따로 확인이 필요하다. 샌드박스 안에서 실행하면 실패가 더 늘어나므로 샌드박스 밖 결과만 적었다

서버 테스트는 이 브랜치가 건드린 세 바이너리만 실행했다. `provider_operations_boundary` 1개,
`human_invite_manager_boundary` 7개는 통과했다. `persona_snapshot_capacity` 는 DB 를 임시 폴더에
만드는데, Windows 의 `%TEMP%` 는 다른 계정 권한을 상속하므로 "DB 폴더는 현재 사용자 전용"
검사(`UnsafeDatabasePath`)에서 DB 를 열기 전에 실패한다. 이 브랜치가 그 파일에 추가한 카탈로그
필드와는 관련이 없어 보이며, 같은 방식으로 임시 폴더에 DB 를 만드는 다른 서버 테스트도 Windows
에서 같은 이유로 실패할 가능성이 높다. 전체 서버 테스트는 실행하지 않았다.

## 남은 진단 과제

강제 종료 뒤의 "복구 필요": 에이전트가 실행 중일 때 앱을 강제 종료(`Stop-Process -Force`)하면, 다시
켰을 때 그 세션이 `runtime_authority_uncertain`("Provider runtime authority could not be confirmed.")
으로 "복구 필요" 가 된다. Windows 에서는 서버가 provider Job 을 직접 들고 있어서, 서버가 강제로 죽으면
런타임 lease 에 `gone` 영수증이 기록되지 않고 `windows-active` 표시만 남는다. 설계 문서는 이 경우를
의도적으로 "알 수 없음" 으로 둔다(Unix 에서 감시 프로세스 없이 남은 표시와 같은 취급). 확인해 보니 실제
provider 프로세스는 Job 종료로 정리돼 있었지만, 앱의 중지 버튼도 `The provider runtime is not owned by
this supervisor.` 로 실패해 UI 에서 풀 방법이 없었다. 창을 정상적으로 닫은 경우에는 세션이 "중지됨"
으로 정리되는 것을 확인했다. 크래시나 정전에서도 같은 상태가 될 수 있으므로, Windows 에서 이 상태를
푸는 경로는 소유자 요청으로 열었다(아래).

앱에서 푸는 방법(12): "복구 필요" 상태의 세션에서 중지 버튼이 "복구" 로 바뀐다. 소유자가 이 버튼을
누르면 서버가 provider 에게 이전 소유자의 소실 증명을 요청한다. 증명 조건은 이 supervisor 가 그 세션을
소유하고 있지 않을 것, 핸들이 같은 launch 토큰의 Windows 핸들일 것, lease 파일이 잠겨 있지 않고
`windows-active:<같은 토큰>` 일 것이다. 모두 맞으면 `gone:<토큰>` 영수증을 적고, 이어지는 중지가 평소대로
끝난다. 증명되지 않으면 아무것도 바꾸지 않고 기존과 같은 불확실 상태로 남는다. 자동 복구 경로는 그대로
두었다. 즉 이 확인은 사용자가 누를 때만 실행된다.

근거: Windows 는 provider Job 을 서버가 직접 들고 있고 Job 은 kill-on-close 로 만들어진다. 서버가 어떻게
죽든 커널이 Job 핸들을 닫으면서 구성원을 모두 종료하며, 같은 소유자가 lease 의 배타 잠금도 들고 있었다.
따라서 잠기지 않은 `windows-active` 표시는 그 세대가 끝났다는 뜻이다. 실행 중인 세션을 강제 종료한 뒤
패키지 앱에서 복구를 눌러 "중지됨" 으로 정리되는 것을 확인했다.

수동으로 푼 방법(앱 기능 아님, 안전장치를 사람이 대신 푸는 것): 앱을 정상 종료하고, 남은 AgentsAssemble ·
브리지 프로세스가 없는 것을 확인한 뒤, 해당 세션의 lease 파일
(`%LOCALAPPDATA%\Temp\agentsassemble-provider-runtime-local\<sha256(room, session)>.lease`) 내용을
`windows-active:<토큰>` 에서 같은 토큰의 `gone:<토큰>` 으로 바꿨다. 토큰은 DB 에 저장된 세션의
`runtime_lease_token` 과 일치하는지 먼저 확인했다. 앱을 다시 켜자 세션이 "중지됨" 으로 정리됐고, 재개하자
대기 상태로 정상 기동했다. 프로세스가 실제로 남아 있는데 이렇게 바꾸면 같은 에이전트가 두 번 실행될 수
있으므로, 프로세스 부재를 확인하지 않은 채 쓰면 안 된다.


GPT Pro 리뷰가 지적한 오류 보존 문제는 이 브랜치에서 고치지 않았다.

- `acp_client.rs` 는 ACP `session/new` · `session/load` 실패를 일반 `protocol_error()` 로 바꿔
  인증 거부인지 세션 거부인지 구분을 잃는다.
- `reject_recovered_start()` 는 세션의 공개 `last_error` 를 복구 문구로 덮어쓴다. 살아 있는 서버의
  reconciliation 도 같은 경로를 쓰므로, 실제 재시작이 없어도 그 문구로 끝날 수 있다.

8-1 을 고치면서 이번 증상은 사라졌지만, 다음 provider 실패에서도 같은 진단 비용을 치르게 된다.
최초 실패 원인과 정리 결과를 분리해 보존하는 것이 다음 과제다.
