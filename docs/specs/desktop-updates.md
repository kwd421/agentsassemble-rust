# Desktop application updates

The user approved Tauri's official updater for macOS and Windows (2026-10-02).
Currently neither installed app checks for application updates. Provider CLI
updates are separate and remain unchanged.

The native desktop owns checking, downloading, signature verification and install.
It checks on launch and every six hours while open, plus a native menu action.
One operation runs at a time. Downloaded updates are offered for explicit restart;
declining preserves the current app and never interrupts rooms or agents. Native
menus remain available when the main WebView visits a remote owned server. No
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
