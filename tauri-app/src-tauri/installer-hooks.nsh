; Windows Firewall rules for Vocalis's LAN ports (step 9 of the Tauri migration — "режим кабинета").
;
; Without this, the *first* time a lesson actually tries to use each port, Windows Firewall pops its own
; "разрешить доступ?" prompt right in front of whoever's at the keyboard — on a student's machine, that's a
; child being asked a security question they have no way to answer correctly, and on a first real lesson no
; less. Pre-authorizing the real ports this app actually listens on (see `common/src/protocol.rs` — the
; full list is also documented in this project's own network report) at install time means that prompt
; never has a reason to appear.
;
; Two rules, not nine: Windows Firewall rules can span a port *list* (commas) and *ranges* (dashes) in one
; rule, so this is one TCP rule (the control channel, 47991) and one UDP rule (everything else —
; discovery, mic/intercom/screen-demo audio and video, 47990 + 47992-47998) rather than nine separate ones.
;
; Rule names are plain ASCII on purpose (not the otherwise-Russian UI the rest of this installer uses) —
; CI verifies these rules really appear via `netsh advfirewall firewall show rule name="..."`, and an exact
; string match across this .nsh file's own encoding, whatever Windows Firewall actually stores it as, and a
; separate PowerShell verification script is one more place a Cyrillic round-trip could silently mismatch;
; plain ASCII removes that risk entirely rather than hoping all three agree.
;
; Deliberately *not* scoped to this app's own .exe via `program=`: Tauri's NSIS bundler doesn't guarantee
; the installed binary keeps a specific, predictable filename inside `$INSTDIR` (unlike this project's
; earlier, short-lived custom Inno Setup installer, which chose that name itself) — getting that wrong
; would silently make the rule match nothing at all, which is a much worse failure than "slightly broader
; than strictly necessary". A plain port+protocol+profile rule is simple, self-contained, and either way
; this app is the only thing with any real reason to listen on these specific nine ports.
;
; `profile=private,domain` only, never `public` — a school's own staff/guest Wi-Fi is "private" or
; "domain" in Windows' own classification; leaving `public` closed is the standard, conservative default
; for an unencrypted "any device on this port can talk to me" rule.
;
; `nsExec::ExecToLog` (not plain `ExecWait`): ships with every standard NSIS distribution (no extra plugin
; download), runs `netsh` without flashing a console window in front of the person installing, and logs
; its real output into the installer's own log for troubleshooting.

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Открываю порты Vocalis в брандмауэре Windows (для частных и доменных сетей)..."
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Vocalis TCP 47991" dir=in action=allow protocol=TCP localport=47991 profile=private,domain'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Vocalis UDP 47990 47992-47998" dir=in action=allow protocol=UDP localport=47990,47992-47998 profile=private,domain'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "Удаляю правила брандмауэра Vocalis..."
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Vocalis TCP 47991"'
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Vocalis UDP 47990 47992-47998"'

  ; Defensive, belt-and-suspenders cleanup for autostart (`device_role.rs`'s `configure_autostart`):
  ; `--set-role=teacher`/`--set-role=none` already removes this normally, but an uninstall could happen
  ; without that ever having been run first (a machine still pinned to "student" when it's uninstalled).
  ; `auto-launch`'s own `WindowsEnableMode::System` writes exactly this value — same name, same key.
  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "Vocalis"
!macroend
