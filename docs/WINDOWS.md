# Windows 런타임

상태: `windows-runtime-fixes` 브랜치 작업 기록. 2026-09-16, Windows 11 26200 / rustc 1.98.1 /
Node 24.13.1 에서 확인했다.

## 요약

Windows에서 런타임이 기동한 적이 없었다. 세 건의 결함이 모두 부팅 경로에 있었고, 셋 다
컴파일과 clippy를 통과하는 종류라 게이트에 걸리지 않았다. 수정 후 데스크톱 앱이 사이드카를
띄우고 `{"status":"ready","runtime":"rust"}` 를 보고하는 것까지 확인했다.

## 왜 드러나지 않았나

`.github/workflows/windows-contract.yml` 은 세 가지만 실행한다.

- `cargo check --locked -p agentsassemble-provider -p agentsassemble-server --all-targets`
- `cargo clippy ... -- -D warnings`
- `cargo test -p agentsassemble-provider managed_bridge`

파일 기반 데이터베이스를 여는 경로, 프론트엔드 릴리스를 만드는 경로, 데스크톱 셸을 실행하는
경로는 어느 것도 실행되지 않는다. `docs/VERIFICATION.md` 의 패키지 검증은 전부 macOS 서명
빌드 기준이다. 즉 Windows는 "컴파일된다"까지만 검증된 상태였다.

## 고친 것

### 1. 파일 DACL을 권한 없는 핸들로 적용 (`crates/agentsassemble-persistence/src/private_fs.rs`)

증상: 서버가 기동 즉시 `writer lease operation failed: 액세스가 거부되었습니다. (os error 5)`
로 종료. 0바이트 `runtime.sqlite3` 만 남는다.

원인: `secure_file` 이 호출자의 `File` 핸들로 DACL을 썼다. 그 핸들은
`OpenOptions::new().create_new(true).read(true).write(true)` 의 산출물이라
`GENERIC_READ | GENERIC_WRITE` 만 가진다. `WRITE_DAC` 가 없으므로 `SetSecurityInfo` 는 항상
`ERROR_ACCESS_DENIED` 를 돌려준다. 디렉터리 하드닝은 이미 경로 기반이라 정상이었다.

수정: 소유자 전용 DACL을 경로로 적용한다. `windows_acl` 이 필요한 권한으로 직접 연다.
`validate_database_file`, `acquire_writer_lease`, `write_key`, `validate_key_file`,
`HostKeyMaterial::read` 가 경로를 함께 넘기도록 시그니처가 바뀌었다. unix 는 기존 핸들 기반
`chmod 0600` 을 그대로 쓰고 추가된 경로 인자를 사용하지 않는다.

트레이드오프: 이제 핸들이 아니라 이름으로 ACL을 건다. 보호는 기존의 심링크 거부,
하드링크 카운트 검사, `PreparedDatabase::revalidate` 의 신원 재확인에 의존한다. 커밋 전
이 부분은 검토가 필요하다.

### 2. 데스크톱 셸의 동일 결함 (`desktop/src-tauri/src/private_fs.rs`)

증상: 앱 창은 뜨지만 "로컬 신원 권위를 확인하지 못했습니다" 만 표시되고 사이드카가 실행되지
않는다. `%APPDATA%\app.agentsassemble.rust` 에 로그 파일이 생성됐다 사라진다.

원인: 1번과 같은 결함의 독립된 복제본. `make_private_file` 이 `secure_handle` 을 통해 런타임
로그와 방 목록 캐시의 DACL을 핸들로 썼다. 첫 로그 파일(`runtime.stdout.log`) 생성 직후
거부되어 `start_runtime` 이 중단된다.

수정: `secure_file_path` 를 추가하고 두 호출부가 경로를 넘긴다. 두 곳 모두 이미 경로를
가지고 있었다.

같은 보안 메커니즘이 두 크레이트에 각각 구현되어 있었다는 점은 `AGENTS.md` 의 "동일한 보안
메커니즘은 구현 소유자가 정확히 하나" 규칙에 걸린다. 한쪽만 고치면 다른 쪽에서 같은 증상이
다시 난다.

### 3. 확장 경로 비교 실패 (`crates/agentsassemble-server/src/frontend_document.rs`)

증상: DB는 열리지만 `frontend document cannot be bound to its release` 로 기동 실패. 어떤
프론트엔드 빌드를 넘겨도 동일하다.

원인: 릴리스 루트는 `canonicalize` 를 거쳐 오는데 Windows 에서 이 함수는 항상 verbatim
(`\\?\C:\...`) 경로를 돌려준다. 반면 각 자산 경로는 `Url::from_directory_path` →
`to_file_path` 왕복을 거쳐 평범한 `C:\...` 형태로 돌아온다. 두 경로를 `starts_with` 로 그대로
비교하므로 **항상 false** 가 되고, 모든 자산이 "missing asset" 으로 거부된다.

수정: Windows 에서만 verbatim 접두사를 제거한 뒤 비교한다. 다른 타깃은 이전과 동일한 경로를
그대로 비교한다.

## 고치지 않은 것

### 첨부 저장 (`desktop/src-tauri/src/message_attachment_save/secure_replace.rs:165`)

`secure_staging_file` 도 핸들 기반 하드닝이라 Windows 에서 같은 `os error 5` 가 난다. 다만
이 경로는 `cap_std` 능력 핸들만 보유하고 경로를 갖지 않는다. 경로를 넘기면 capability 기반
설계 자체가 바뀌므로 소유자 결정이 필요하다. **Windows 에서 첨부 저장은 실패한다.**

### 승인 CSS 해시 게이트 (`frontend/scripts/verify-original-css.mjs`)

승인 목록이 macOS 빌드 산출물(`index-CXR-yvE9.css`)에 고정되어 있다. Windows 에서 같은 소스로
빌드하면 다른 해시(`index-DOgTJZgg.css`)가 나온다. CRLF 는 원인이 아니다 — LF 로 정규화해
다시 빌드해도 동일하다. Tailwind 4 의 네이티브 엔진이 플랫폼별로 다른 출력을 내는 것으로
보인다.

결과적으로 `npm run build`, `make test`, 그리고 Tauri 의 `beforeBuildCommand` 가 Windows 에서
**구조적으로 실패한다.** 아래 실행 절차는 이 검사를 건너뛴다.

### 네이티브 실패 원인이 UI에서 사라짐 (`frontend/src/views/components/StartupIdentityGate.tsx:159`)

`reason instanceof Error` 가 참일 때만 구체적 메시지를 표시한다. Tauri 커맨드는 전부
`Result<_, String>` 이라 네이티브 실패는 **항상 문자열로 reject** 되고, 그래서 정확히 네이티브가
실패했을 때만 원인이 버려지고 "로컬 신원 권위를 확인하지 못했습니다" 만 남는다. 2번 결함의
진단에 가장 많은 시간이 들어간 이유다.

## 외부 AI 초대는 공개 접속 없이는 불가능하다

같은 PC 에서 돌고 있는 외부 AI 를 초대하는 경우에도 마찬가지다. UI 제약이 아니라 서버가
거부한다.

- `crates/agentsassemble-server/src/connector_invite_manager_web.rs:47` — Room Connector 초대는
  `public_ingress.ready_snapshot()` 이 없으면 `public_ingress_not_ready` 로 거부한다.
- `crates/agentsassemble-server/src/attendee_entry_web.rs:142` — AgentBridge 초대도 동일하게
  409 CONFLICT.
- 두 경로 모두 join URL 을 `ingress.public_url` 로 조립한다. 공개 오리진이 없으면 만들 문자열
  자체가 없다.

수동 모드로도 우회되지 않는다. `AGENTSASSEMBLE_PUBLIC_URL` 은 canonical non-loopback HTTPS
오리진만 받는다(`public_ingress.rs:120`). `http://127.0.0.1:<port>` 는 거부된다. 그리고
`AGENTSASSEMBLE_TRUSTED_PROXY_TOKEN` 과 반드시 함께 설정해야 한다(`main.rs:685`).

관리형 터널은 `cloudflared` 를 PATH 에서 찾는다(`which::which("cloudflared")`). 번들에 포함되어
있지 않으므로 사용자가 직접 설치해야 하고, 없으면 "cloudflared 설치 상태를 확인하세요" 상태가
된다.

### 로컬 AI 를 방에 넣는 것은 별개 경로다

| 경로 | 방식 | 공개 인그레스 |
| --- | --- | --- |
| Agent Session | 앱이 provider 를 직접 실행 (Codex, Claude, DeepSeek, Ollama, LM Studio 등) | 불필요 |
| Room Connector / AgentBridge | 이미 실행 중인 외부 AI 세션이 초대 링크로 입장 | 필요 |

에이전트 생성·실행 경로에는 `public_ingress` 참조가 없다. 방에 AI 를 추가하는 일반 플로우는
터널 없이 동작한다.

## Windows 에서 실행하기

`npm run build` 와 `make test` 는 위의 CSS 게이트 때문에 실패하므로, Tauri 설정만 런타임에
덮어써서 그 단계를 건너뛴다. 저장소 파일은 바꾸지 않는다.

```
# 1. 프론트엔드 (vite build 는 성공한다. 마지막 CSS 검사만 실패하며 dist 는 생성된다)
npm --prefix frontend install
npm --prefix frontend run build

# 2. provider-runtime 리소스와 데스크톱 의존성
npm --prefix provider-runtime ci --omit=optional --ignore-scripts
npm --prefix desktop install

# 3. 사이드카 (서버 + 슈퍼바이저)
npm --prefix desktop run prepare:sidecar

# 4. 패키징 모드로 빌드. beforeBuildCommand 를 비운 설정 파일을 넘긴다
#    {"build": {"beforeBuildCommand": ""}}
npx --prefix desktop tauri build --debug --no-bundle --config <override>.json

# 5. 리소스를 실행 파일 옆에 배치한 뒤 실행
#    target/debug/frontend/          <- frontend/dist 내용
#    target/debug/provider-runtime/  <- provider-runtime 의 .mjs 와 sdk.mjs
target/debug/agentsassemble-desktop.exe
```

`tauri dev` 는 쓰지 말 것. dev 서버가 `http://127.0.0.1:1430` 에서 UI 를 서빙하는데,
`caller_is_bundled_ui` 는 `tauri://localhost` 와 `tauri.localhost` 만 허용하므로 모든 네이티브
커맨드가 거부된다. 이건 Windows 문제가 아니라 dev 모드와 패키징 모드의 차이이며 macOS 에서도
동일하다.

## 검증되지 않은 것

기동까지만 확인했다. 아래는 이 브랜치에서 실행해 본 적이 없다.

- 방 생성, 에이전트 추가, 실제 provider 턴
- 공개 인그레스(cloudflared 터널) 전체 경로. 정적으로는 Windows 대응이 되어 있다 —
  `which` 가 PATHEXT 를 처리하고, `owned_command` 가 JobObject 로 자식을 소유하며,
  `request_graceful_stop` 에 `cfg(not(unix))` 분기가 있다. 실행 검증은 없다.
- 롤링 재시작. `runtime_reexec::InheritedListeners` 는 `cfg(unix)` 전용이다.
- 첨부 저장. 위 참조대로 실패가 예상된다.
