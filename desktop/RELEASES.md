# Desktop updates

macOS and Windows use the official Tauri updater from the native Update menu.
The app checks at launch and every six hours, downloads and verifies a newer
release, then asks before stopping its server and installing. Declining retains
one verified artifact in memory until installation or app exit. Browsers and remote
WebViews have no update installation commands. Version-bound signatures and HTTPS
are required; downgrades remain disabled.

## Signing ownership

`src-tauri/tauri.conf.json` contains the public verification key. The corresponding
private key is stored locally at `~/.config/agentsassemble/release/updater.key`
(private directory/file permissions) and in this repository's GitHub Actions
secret `TAURI_SIGNING_PRIVATE_KEY`. Back up that key securely: losing it prevents
existing installations from accepting future releases. Never commit it or pass it
on a command line. Updater signatures are distinct from Apple Developer ID signing,
notarization and Windows Authenticode signing.

## Publish a version

1. Advance `src-tauri/tauri.conf.json` to a new SemVer and commit the verified change.
   Initial release targets are Apple Silicon macOS and x86-64 Windows.
2. Push tag `desktop-v<VERSION>` at that commit. The Windows release workflow builds
   the current-user NSIS installer, runs desktop tests and uploads the installer
   with its version-bound `.sig`. A failed build/test does not publish a release.
3. On the Mac, use the existing Developer ID identity and updater key to build:

   ```sh
   export APPLE_SIGNING_IDENTITY='<configured Developer ID identity>'
   export TAURI_SIGNING_PRIVATE_KEY="$HOME/.config/agentsassemble/release/updater.key"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=''
   export CARGO_TARGET_DIR="$(git rev-parse --show-toplevel)/target"
   npm run build:signed:macos
   ```

   On the current rustc 1.97.1 / LLVM 22 / Xcode 27 host, set
   `CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=none` for the build: stripping
   proc-macro dylibs triggers upstream rust-lang/rust#157750 (misaligned LINKEDIT).
   This affects build-time dependencies, not runtime update verification.

   Configure Apple notarization credentials when distributing publicly. The build
   reports whether notarization was performed; a signed bundle alone is not
   notarization proof. Verify the packaged app and its user flow before publishing.
4. Download `windows-x86_64-updater` from the successful workflow run for the exact
   tagged commit. Stage both platform artifacts and signatures, then run:

   ```sh
   node scripts/release_manifest.mjs /path/to/latest.json \
     darwin-aarch64=/path/to/AgentsAssemble.app.tar.gz \
     windows-x86_64=/path/to/AgentsAssemble_0.1.3_x64-setup.exe
   ```

5. Create a draft GitHub Release for that tag. Upload both artifacts, their `.sig`
   files and `latest.json`; confirm every manifest URL matches an attached asset.
   Publish and mark it latest only after all advertised assets are ready. Check
   the public `releases/latest/download/latest.json` and perform an actual update
   from the previous version. Do not edit an already-published version's binaries.

Ordinary source pushes do not publish updates. Existing installations without the
updater require one manual installation of an updater-enabled release. Keep app
data outside the bundle and preserve `app.agentsassemble.rust` across releases.
