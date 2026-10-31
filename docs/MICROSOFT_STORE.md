# Microsoft Store packaging

The Windows Store edition packages the existing Rust/Qt desktop client as an
x64 MSIX. It is a full-trust desktop app, not an AppContainer or a PWA. The
manifest targets Windows Desktop, Windows 10 build 19041 or later. This target
is not evidence of acceptance on every supported OS or processor.

The Store listing, product `9N09Q23FW92M`, is the official Windows release.
Microsoft signs that package and delivers its updates. SignPath signing of the
GitHub Windows artifacts waits until SignPath accepts the project. That program
requires proof of an existing trusted distribution, and this Store release is
that proof. Until then, GitHub Windows builds stay unsigned.

## Build the upload

Run from the repository root:

```powershell
.\build_msix.ps1
```

Requirements are those of [build_win64.ps1](../build_win64.ps1), plus Windows
SDK MakeAppx.exe, Visual Studio C++ redistributable DLLs and .NET System.Drawing.
The script builds/deploys the current client through the portable build, skips
the unrelated `.7z`, and creates:

```text
dist/msix/DoubleSlash-1.0.<build>.0-x64.msix
dist/msix/DoubleSlash-1.0.<build>.0-x64.msix.sha256
```

The package version is the product version from `scripts/app_version.ps1`:
`{major}.{minor}.{build}.0`. Major and minor come from the client Cargo version.
The build is the git commit count, shared with the desktop binary, the installer,
and Android `versionName`. The fourth component stays 0 because the Store
reserves it. `-SkipBuild` reuses `dist/DoubleSlash` and is for packaging
iteration after building these sources, not stale release binaries.
`-MakeAppxPath` overrides SDK discovery. The release workflow packs this MSIX
after `build_win64.ps1` and uploads it unsigned. Microsoft signs it after
certification. The workflow does not submit it to Partner Center.

The manifest uses the values assigned in Partner Center:

| Field | Value |
|---|---|
| Package identity name | `DoubleSlash.DoubleSlash` |
| Package publisher | `CN=500F9E8C-187C-408B-8700-5C8A6D6C7B1C` |
| Publisher display name | `Double Slash` |

MakeAppx validates the manifest. The final MSIX is independently extracted by
the distribution-notice verifier. It contains client notices, Qt/WebEngine and
FFmpeg, app-local Visual C++ runtime DLLs, and logos derived from the existing
project icon. The icon's own 16–256 px frames ship as unplated `targetsize`
variants, indexed in `resources.pri`, so the taskbar draws the icon without
an accent-coloured plate. It excludes the standalone updater and runtime setup executables.
The LGPL replacement instructions explain running an extracted, writable copy
with substituted dynamic libraries; users do not need to modify WindowsApps.

The output is unsigned and intended for Partner Center's **Packages** upload.
Microsoft signs it after certification. Building does not publish it or grant
local certificate trust. For local deployment, use a separately test-signed copy
or a development registration; never install a private signing key into a
customer's trust store.

## Runtime behavior

`platform::is_windows_package` queries `GetCurrentPackageFullName`. A packaged
process skips GitHub update requests and refuses installer handoffs, even if the
saved automatic-check setting is enabled. Windows owns Start-menu registration
and `doubleslash://` associations through the manifest; the app
does not overwrite or delete the unpackaged edition's registry handlers or
shortcuts. First-run setup omits those integration options in an MSIX process.
MSIX requires protocol names of at least two characters, so it cannot register
the one-letter `d://` scheme with Windows. The native parser and in-app portal
retain that alias; external browser invite activation uses `doubleslash://`,
as the existing HTTPS invite landing page already does.

The package does not redirect, migrate or bundle identity/profile files.
`Identity::default_key_dir()` still resolves the selected desktop profile from
`DOUBLESLASH_KEY_DIR` / `DOUBLESLASH_HOME` or `~/.doubleslash`. Keep the existing
one-process-per-profile rule when switching between Store and portable copies.
Uninstalling the package is not an identity deletion action. Use the client's
encrypted `.dbackup` export/restore for intentional profile moves.

Camera, microphone and network capabilities are declared. They do not substitute
for Windows privacy permissions or imply that media flows passed acceptance.
No identity authority or runtime dependency on Store authentication is added.

## Submission and acceptance

In Partner Center select **Windows 10/11 Desktop** only. Upload the MSIX, retain
the checksum locally, and supply clear instructions for reviewers to create an
identity and test invite-based communication against functioning peers/nodes.
Use the exact feature declarations for the Windows build, including optional
Ollama generative AI. Do not claim accessibility certification, chat moderation,
or an in-app report flow that has not been implemented and validated.

Outstanding acceptance is tracked in [backlog.md](../backlog.md). At minimum,
validate package installation and update, alternate-volume installation,
profile/credential preservation, first launch and invite activation with the app
closed and open, Qt/WebEngine, privacy controls, direct/room media and files,
notifications/tray, and uninstall/reinstall. Store content/reporting requirements
need review before publication. A schema-valid package is not a passed Windows
App Certification Kit run or a Store-approved submission.

References: [MSIX package requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements),
[protocol extensions](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/desktop-to-uwp-extensions),
[Store policies](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies).
