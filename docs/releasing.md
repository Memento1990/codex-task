# Safe GitHub releases

A request to “发版” / “发布版本” means a GitHub Release for `Memento1990/codex-task`, not installation. Use [publish-release.ps1](../app/desktop/publish-release.ps1) in two phases. Do not overwrite an existing tag, release, or asset. Nothing here installs into production, replaces a database, stops a process, or connects a development account.

## Prerequisites

- Windows, PowerShell 7+, Node, pnpm, Cargo, the repository's local MSVC/Windows SDK setup used by [build-native.ps1](../app/desktop/build-native.ps1), and cached dependencies for offline lock regeneration/install.
- An authenticated GitHub CLI with release write permissions. `-GhPath` accepts an executable path, for example `D:\temp\floatboard-release-tools-20261103\gh\bin\gh.exe`. Proxy environment variables are inherited; no credentials/proxy settings are written.
- Existing signing credentials usable by `FloatboardRelease.exe sign`. Never print, copy into the repository, or generate a replacement private key to work around a failure. [update-public-key.txt](../app/desktop/update-public-key.txt) is the pinned trust anchor.
- Checked-out `main`, expected GitHub `origin`, empty index, and remote `main` exactly equal to local HEAD. Reconcile divergent/local unpublished commits first. Stable version strictly greater than the committed HEAD package version; no prerelease syntax.
- Nonempty release notes. Explicitly enumerate **files**, not directories, for every intentional tracked modification and application-source addition. Renames require both old/new paths. Never use `git add .`.

## 1. Prepare (local writes, read-only remote preflight)

Run from repository root. This example is illustrative; select the next version and your actual approved source files. The notes file may be outside the repository; if it belongs in the release commit, include it explicitly.

```powershell
$version = '2.0.0'
$notes = 'docs/release-2.0.0.md'
$stage = @(
  'app/desktop/publish-release.ps1',
  'app/desktop/verify-release.cjs',
  'docs/releasing.md',
  'docs/release-2.0.0.md'
  # Enumerate each other reviewed feature/test/documentation file here.
)
$prepared = 'D:\temp\floatboard-release-2.0.0-unique-run'
$gh = 'D:\temp\floatboard-release-tools-20261103\gh\bin\gh.exe'
./app/desktop/publish-release.ps1 -Version $version -NotesFile $notes `
  -StagePaths $stage -PreparedDirectory $prepared -GhPath $gh
```

Prepare refuses unrelated tracked dirt and unapproved untracked application files; unrelated untracked research/design files are not staged. It checks whitespace in tracked changes and every allowlisted new file before building, without staging them. Version metadata and the unified Cargo and pnpm lockfiles are automatically allowlisted. It updates package/Tauri JSON and all six workspace package versions, updates pnpm/Cargo lockfiles **offline** (Cargo metadata preserves existing dependency resolutions), installs frozen offline dependencies without lifecycle scripts, then runs frontend tests/check/build, core Rust tests, all native Rust tests, Agent/client/maintenance/installer/launcher/updater/release-signer build and Tauri release build. Core test preparation explicitly builds its Agent/client/shell process fixtures in the new target before running installer tests, so a clean target never relies on old local binaries.

Every run requires a new output directory outside the repository and `LOCALAPPDATA`, with its own `target` directory. Live Reporter executables in historical targets are neither overwritten nor terminated. Failed preparation may leave version/lock edits and incomplete external outputs: inspect them; do not blindly reset unrelated work. A retry requires a new directory; already bumped working-tree version edits can be retained because the version increase is checked against committed HEAD.

[package-release.ps1](../app/desktop/package-release.ps1) checks executable build identities, packages and signs. The independent built-in-Node-only [verify-release.cjs](../app/desktop/verify-release.cjs) verifies the raw manifest Ed25519 signature against the pinned key, exact stable manifest/version/platform/URL/protocol fields, update ZIP size/hash, and every expected checksum entry. All five asset sizes/hashes and a source inventory digest are frozen in `prepared.json` outside the repository. This receipt is local evidence, not a signed attestation: keep the preparation directory trusted.

## 2. Review and isolated smoke

Inspect notes, source diff, regenerated locks, and package contents. Review lockfile diffs, which should normally only update the root package version. Cargo metadata preserves existing dependency resolutions; any dependency change must be understood before publication. ZIP safety/content still deserves human review: the independent helper verifies authenticity and bytes, not archive semantics. It never executes binaries.

Launch **only** via the existing DEV wrapper:

```powershell
./app/desktop/dev-isolated.ps1 `
  -Executable "$prepared\target\release\CodexFloatboard.exe" -CodexMode isolated
node ./app/desktop/verify-release.cjs "$prepared\assets" $version `
  ./app/desktop/update-public-key.txt
```

Check the DEV indicator, release features/windows, and no production changes. Close this DEV GUI normally when done; do not kill occupied Reporter processes. `-SmokeReviewed` below is your explicit assertion of this manual review, not an automated substitute.

## 3. Publish (explicit remote permission)

```powershell
./app/desktop/publish-release.ps1 -Mode Publish -Publish -SmokeReviewed `
  -Version $version -NotesFile $notes -PreparedDirectory $prepared -GhPath $gh
```

Publish does not rebuild or silently bump again. It repeats branch/origin/remote/tag/release/index checks, rehashes tracked and explicitly included source, notes and assets, and refuses stale packages. It stages only the frozen explicit file list, commits, validates the resulting source/commit ancestry and approved diff, makes an annotated tag, and atomically pushes `main` plus the new tag. It creates a **draft** with all five assets; only after GitHub-reported sizes and SHA-256 digests match does it publish and mark latest, then check latest identity. Both modes require authenticated read-only remote queries; only `-Mode Publish -Publish -SmokeReviewed` enables remote mutation.

Never bypass an error with force push, `--clobber`, or replacement assets. A failure after commit/tag/push may intentionally leave a local tag, remote tag or unpublished draft. Automatic restart is refused. Inspect remote/local state and preserve evidence; coordinate a new version or separately reviewed draft completion. There is no destructive rollback/resume logic. Git hooks/remote permissions and atomic push support may cause a safe stop. Do not modify sources concurrently during release; source hashes catch ordinary changes, not malicious concurrent actors or a compromised local toolchain.

The published release is not installed automatically. The installed app checks/downloads and verifies the GitHub release; installation still requires explicit user confirmation. Production data/configuration and running Codex/Reporter sessions remain untouched.

## Lightweight automation checks (no real build/release)

```powershell
$tokens = $null; $errors = $null
[System.Management.Automation.Language.Parser]::ParseFile(
  (Resolve-Path ./app/desktop/publish-release.ps1), [ref]$tokens, [ref]$errors) | Out-Null
if ($errors.Count) { throw ($errors | Out-String) }
node --check ./app/desktop/verify-release.cjs
node ./app/desktop/verify-release.cjs --self-test
```

The helper's temporary generated-key fixtures accept a valid package and reject byte tampering, rehashed tampering, and an invalid signature. These fixtures are deliberately not ZIP parsing tests. No build, version mutation, signing-key access or GitHub publication occurs in this check.
