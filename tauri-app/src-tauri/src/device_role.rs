//! "Режим кабинета" (step 9 of the Tauri migration): a role pinned to *this machine's installation*, not
//! chosen by whoever is sitting at it. Real school deployment, not a hypothetical: lab laptops are
//! installed once, then reused by a different class every lesson — the student side should never show a
//! "Учитель/Ученик" picker at all, and a child must not be able to flip it over by tapping the wrong
//! button, while IT still needs a real way to set or change it, including in bulk before cloning an image
//! to 15 machines.
//!
//! **Where the role lives, and why**: a plain text file (`role.txt`, containing exactly `teacher` or
//! `student`) *inside the app's own Program Files install directory*, right next to the `.exe` — not
//! `%ProgramData%`, not `%AppData%`. That's deliberate: Program Files is the one location this installer
//! already places files in that Windows itself makes read-only for standard user accounts by default — a
//! real student Windows login cannot write there, full stop, with zero custom ACL code to get right or
//! maintain. A config file in `%ProgramData%` would need its own explicit, hand-rolled ACL lockdown to get
//! the same guarantee (that directory's default permissions are more permissive); reusing Program Files'
//! own, OS-enforced protection is simpler and strictly safer.
//!
//! **How it's set**: `Vocalis.exe --set-role=teacher|student|none` — a real CLI mode, handled in `main.rs`
//! before Tauri or any window ever starts. Writing the role file needs the same admin elevation the
//! install itself already required (Program Files, `installMode: "perMachine"` — see `tauri.conf.json`),
//! so this can't be triggered by the student account that normally uses the machine, only by IT running it
//! elevated — once per machine, or once on a reference image before cloning it to the rest of the lab.
//! Setting `student` also registers the app for real Windows autostart (`configure_autostart` below);
//! setting `teacher` or `none` un-registers it. `none` returns the machine to today's behavior — the
//! Учитель/Ученик picker, chosen fresh every launch — so this is never a one-way door.
//!
//! **How it's read**: `get_device_role()` (`commands/device_role.rs`), called once by the frontend
//! (`App.tsx`) on startup, before it ever renders the role picker.

use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Teacher,
    Student,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Teacher => "teacher",
            Role::Student => "student",
        }
    }

    fn parse(s: &str) -> Option<Role> {
        match s.trim() {
            "teacher" => Some(Role::Teacher),
            "student" => Some(Role::Student),
            _ => None,
        }
    }
}

/// `role.txt`, next to the running `.exe` — see this module's own doc comment for why that location
/// specifically. Resolved fresh each call (not cached) since it's only ever read/written a handful of
/// times per process lifetime — once at frontend startup, or once for a whole `--set-role` CLI invocation.
fn role_file_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::other("the running exe has no parent directory"))?;
    Ok(dir.join("role.txt"))
}

/// The role pinned to this machine, if any — `None` for "not pinned" (no file, or one that failed to read
/// or parse, treated the same: fall back to today's picker rather than erroring the whole app over this).
pub fn read_role() -> Option<Role> {
    let path = role_file_path().ok()?;
    let contents = std::fs::read_to_string(path).ok()?;
    Role::parse(&contents)
}

/// `role`: `None` clears the pin (deletes the file) and returns to the Учитель/Ученик picker; `Some(_)`
/// writes it. Also flips real Windows autostart to match (see `configure_autostart`) — the two are always
/// kept in sync by this one function, so there's no way to end up with autostart on but no pinned student
/// role, or vice versa.
pub fn write_role(role: Option<Role>) -> io::Result<()> {
    let path = role_file_path()?;
    match role {
        Some(r) => std::fs::write(&path, r.as_str())?,
        None => {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
    }
    configure_autostart(role == Some(Role::Student))
}

/// Registers (or un-registers) the app to launch automatically when Windows starts — real machine-wide
/// autostart (`WindowsEnableMode::System`, i.e. `HKEY_LOCAL_MACHINE\...\Run`), not tied to whichever
/// Windows account happened to be logged in when `--set-role=student` ran, since a lab machine may have
/// more than one student account logging into it over time. Writing `HKEY_LOCAL_MACHINE` needs admin
/// rights — the same elevation `write_role` above already needs for its own file, so this never grants a
/// capability a non-admin `--set-role` invocation wouldn't already have been refused for.
///
/// A plain `auto_launch::Result` error here (e.g. "not elevated") deliberately does *not* stop the role
/// file itself from having been written successfully just above — a teacher-mode or unpinned machine
/// disabling autostart failing silently is harmless (there was nothing relying on it), and a student-mode
/// machine where enabling it failed still works correctly by hand, just without the "already running at
/// login" convenience; `--set-role`'s own caller (see `handle_cli` below) still surfaces this as part of
/// its result so it's not silently swallowed from the person running the command.
fn configure_autostart(enable: bool) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let exe_path = exe.to_string_lossy().to_string();

    #[cfg(windows)]
    let built = auto_launch::AutoLaunchBuilder::new()
        .set_app_name("Vocalis")
        .set_app_path(&exe_path)
        .set_windows_enable_mode(auto_launch::WindowsEnableMode::System)
        .build();
    #[cfg(not(windows))]
    let built = auto_launch::AutoLaunchBuilder::new().set_app_name("Vocalis").set_app_path(&exe_path).build();
    let auto = built.map_err(io::Error::other)?;

    let result = if enable { auto.enable() } else { auto.disable() };
    // "Already in the requested state" isn't a real failure for our purposes (e.g. re-running
    // `--set-role=student` on an already-pinned machine) — `auto_launch` surfaces that as an `Err` on some
    // platforms, so only a genuine registry-write failure (not-elevated, mainly) should propagate.
    match result {
        Ok(()) => Ok(()),
        Err(e) if auto.is_enabled().unwrap_or(enable) == enable => {
            let _ = e; // already in the desired state — not a real error
            Ok(())
        }
        Err(e) => Err(io::Error::other(e)),
    }
}

/// Shows a real native dialog — not console output, which the running `.exe` (`windows_subsystem =
/// "windows"` in release builds, see `main.rs`) has no guarantee ever reaches whoever actually ran the
/// command, whether that's an IT technician double-clicking it or a provisioning script's own log capture.
fn show_result_dialog(title: &str, message: &str, is_error: bool) {
    use rfd::{MessageDialog, MessageLevel};
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(if is_error { MessageLevel::Error } else { MessageLevel::Info })
        .show();
}

/// Parses and runs a `--set-role=...` / `--get-role` CLI invocation if `args` (the process's own argv,
/// *excluding* argv[0]) contains one. Returns `Some(exit_code)` when it handled one (the caller — `main.rs`
/// — should exit with that code instead of starting Tauri at all); `None` means "not a device-role
/// command, launch the app normally."
pub fn handle_cli(args: &[String]) -> Option<i32> {
    let arg = args.iter().find(|a| a.starts_with("--set-role") || a.as_str() == "--get-role")?;

    if arg == "--get-role" {
        let current = read_role().map(|r| r.as_str()).unwrap_or("не закреплена (показывается выбор роли)");
        show_result_dialog("Роль устройства Vocalis", &format!("Текущая роль: {current}"), false);
        return Some(0);
    }

    let value = arg.strip_prefix("--set-role=").unwrap_or("").trim();
    let target = match value {
        "teacher" => Some(Role::Teacher),
        "student" => Some(Role::Student),
        "none" => None,
        other => {
            show_result_dialog(
                "Роль устройства Vocalis — ошибка",
                &format!("Неизвестное значение '{other}'. Используйте --set-role=teacher, --set-role=student или --set-role=none."),
                true,
            );
            return Some(2);
        }
    };

    match write_role(target) {
        Ok(()) => {
            let label = target.map(|r| r.as_str()).unwrap_or("не закреплена (выбор роли при запуске)");
            show_result_dialog("Роль устройства Vocalis", &format!("Готово. Роль: {label}."), false);
            Some(0)
        }
        Err(e) => {
            show_result_dialog(
                "Роль устройства Vocalis — ошибка",
                &format!("Не удалось изменить роль устройства: {e}\n\nЗапустите эту команду от имени администратора."),
                true,
            );
            Some(1)
        }
    }
}
