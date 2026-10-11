# Desktop application updates

The user approved Tauri's official updater for macOS and Windows (2026-10-02).
Before this change neither installed app checked for application updates. Provider CLI
updates are separate and remain unchanged.

The native desktop owns checking, downloading, signature verification and install.
It checks on launch and every six hours while open, plus desktop settings and a
non-Windows native menu action.
One operation runs at a time. Downloaded updates are offered for explicit restart;
declining preserves the current app and never interrupts rooms or agents. Native
menus on macOS remain available when the main WebView visits a remote owned server. No
remote page receives updater IPC, endpoint selection or installation authority.

The production feed is HTTPS GitHub Releases metadata. Tauri's mandatory artifact
signature is checked using an embedded public key. Private signing keys stay outside
the repository and are supplied as release secrets. OS code signing remains a
separate distribution concern. Updates use monotonic SemVer, platform-specific
artifacts and a published manifest only after all advertised artifacts exist.
Windows uses a current-user NSIS installer and passive update installation.

Installation requires explicit native confirmation of restart and host interruption.
The runtime owner closes admission, joins its login/runtime supervisors and must
confirm shutdown before replacement. Failure prevents installation and is visible;
retries do not race a runtime restart. Existing user data, credentials, server IDs,
profiles and rooms remain outside the replaceable application bundle. Install
errors are not reported as success. No installer fallback or downgrade is added.

Acceptance: native startup/manual check, no-update/error visibility, duplicate
request exclusion, valid signed download/install/relaunch, invalid signature
rejection, runtime admission closure and shutdown failure blocking. Verify macOS
with a signed packaged app and Windows with the release build/available runner;
distinguish Windows build proof from interactive update proof. Run affected native
tests, Clippy and mandatory gates. Record release URL, tested versions, artifacts
and remaining limitations in docs/VERIFICATION.md. The first updater-enabled
version requires one manual installation on existing devices.

## Windows window chrome and manual check (2026-10-11)

Windows must not attach an application menu. Keep the native decorated window,
caption buttons, resize and snap, the title AgentsAssemble, and WebView editing
shortcuts; set only Windows' native window theme to dark. macOS menu and window
appearance remain unchanged. The updater's status MenuItem may remain unattached
on Windows; it is presentation, never the owner of check/download/install state.
The existing in-app indicator owns the restart/update action.

Add a desktop-only shared-style 업데이트 확인 action in 사용자 설정. The bundled
frontend invokes the existing native check command and shows 최신 버전이에요 or
새 버전 {v} inline only after the native owner completes the check; queued work is
not a successful result. Errors remain visible inline and retryable. Reuse the
single native request queue, signed downloads and existing indicator events; no
new polling, endpoint, storage, or remote-page updater authority. Native menu checks
keep their existing notices. Acceptance: affected updater/UI regressions, desktop
check/clippy, frontend build and mandatory structure gates, then push and dispatch
the Windows test release. Computer Use is explicitly excluded; native appearance,
editing shortcuts and interactive caption/snap behavior remain physically unverified.
