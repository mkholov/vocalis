; Vocalis — Inno Setup installer for the Tauri app (tauri-app/).
;
; Why a custom installer instead of Tauri's own built-in bundler (tauri.conf.json's
; `bundle.targets: "all"`, which already produces an NSIS .exe and an MSI on Windows
; out of the box): Tauri's NSIS default installs per-user, into %LOCALAPPDATA%, with
; no admin prompt (`installMode: "currentUser"` — see the Tauri docs; not overridden
; here) — convenient for a single dev machine, wrong for a school lab: IT typically
; images/pushes software machine-wide, into Program Files, once, for every account
; that logs into that PC. Shipping three different installers (NSIS + MSI + this one)
; for one app would also just be confusing ("which do I download?") for a school with
; no dedicated IT staff. So CI builds the real release .exe the normal way (`npm run
; build` + `cargo build --release`, exactly what `tauri build` does before handing off
; to its own bundler) and this script becomes the one and only thing packaged and
; shipped — see .github/workflows/tauri-windows-build.yml.
;
; No install-time role question (Учитель/Ученик): unlike the older egui app (three
; separate .exe, one per role, so "which role" really was an install-time, which-file
; question), tauri-app is a single binary — every launch opens on `RolePicker`
; (src/App.tsx), which already asks Teacher-or-Student itself, fresh each time. There
; is no second binary, config file, or role-specific asset an installer could
; meaningfully pick between; asking again here would just be the same question one
; screen earlier, not a real install-time choice. If a "always start this PC as
; Student" shortcut ever becomes worth having, that needs the app itself to learn to
; read a launch flag/config file first — real functional code, out of scope here
; ("чисто про упаковку/установку").
;
; WebView2 runtime: assumed already present, exactly like Tauri's own NSIS default
; (`webviewInstallMode` not overridden in tauri.conf.json, so Tauri's installer would
; also just assume/bootstrap it at first run rather than bundling it) — Windows 10
; 21H2+ and Windows 11 both ship it pre-installed and keep it updated via Windows
; Update, which covers the overwhelming majority of real school PCs. Not re-checked or
; bundled here; a genuinely offline/air-gapped lab would need that added separately.

#ifndef MyAppVersion
  ; Fallback for a manual local `iscc` run without `/DMyAppVersion=...` — CI always
  ; passes the real one, read live from ../src-tauri/tauri.conf.json's "version" field
  ; (see the workflow), so this constant and that file can't silently drift apart.
  #define MyAppVersion "0.1.0"
#endif

#define MyAppName "Vocalis"
#define MyAppPublisher "Vocalis"
#define MyAppExeName "Vocalis.exe"
#define MyAppIcon "..\src-tauri\icons\icon.ico"
; The real release binary `cargo build --release` (via `npm run tauri build`, or
; equivalently `npm run build` + `cargo build --release`) produces — plain package
; name, no [[bin]] override in src-tauri/Cargo.toml. Installed under a friendlier
; name (see [Files] below); nothing in the app reads its own exe filename back, so
; renaming it on the way in is safe.
#define SourceExe "..\src-tauri\target\release\tauri-app.exe"

[Setup]
AppId={{B6C3E6C8-6E1B-4C6E-9C7A-6F1B7B6C2A11}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
; Program Files, machine-wide (school IT deploys once per lab PC, for every account
; on it) — the whole reason this script exists instead of Tauri's own per-user NSIS
; default. Requires the installer to be run elevated.
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile={#MyAppIcon}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
OutputDir=Output
OutputBaseFilename=Vocalis-Setup-{#MyAppVersion}
; Not code-signed (no certificate to sign with in this project yet) — Windows
; SmartScreen will warn on first run until this installer builds up reputation or
; gets a real signing certificate. Out of scope for "чисто про упаковку/установку";
; noted here so it isn't mistaken for an oversight.

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
; Checked by default, but still a real checkbox the person installing can clear —
; standard Inno Setup convention for a desktop shortcut, and still satisfies "создаёт
; ярлык на рабочем столе" out of the box without taking that choice away entirely.
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent
