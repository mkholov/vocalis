fn main() {
    // Auto-generates `allow-<command>`/`deny-<command>` permissions for our own
    // (non-plugin) commands — Tauri v2's ACL denies everything by default, even
    // app-defined commands, so `capabilities/default.json` can reference these
    // identifiers. See `list_classes` etc. in `src/commands/`.
    let attributes = tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "list_classes",
            "create_class",
            "rename_class",
            "delete_class",
            "class_stats",
            "list_audio_devices",
            "discover_teachers",
            "capture_screen_preview",
            "start_teacher_session",
            "stop_teacher_session",
            "start_student_mic_meter",
            "stop_student_mic_meter",
            "start_screen_demo",
            "stop_screen_demo",
            "start_own_screen_demo",
            "stop_own_screen_demo",
            "start_mic_broadcast",
            "stop_mic_broadcast",
            "start_listen",
            "stop_listen",
            "start_intercom",
            "stop_intercom",
            "create_group",
            "leave_group",
            "send_assignment",
            "list_materials",
            "upload_material",
            "play_material",
            "stop_playback",
            "start_recording",
            "stop_recording",
            "list_recordings",
            "read_recording",
            "delete_recording",
            "connect_student_session",
            "disconnect_student_session",
            "set_hand_raised",
        ]),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build codegen");

    embed_common_controls_manifest_for_tests();
}

/// Gives the *test* executables the Common Controls v6 manifest `tauri-build`
/// only gives the app's own exe.
///
/// `tauri-build` compiles its Windows resource (icon, version info, and the
/// manifest declaring a Common-Controls-v6 dependency) through
/// `embed_resource::compile`, which emits `cargo:rustc-link-arg-bins=...` — so
/// only the `tauri-app` binary gets it. Test executables get none, and
/// Windows then loads the legacy comctl32 v5, which doesn't export
/// `SetWindowSubclass`/`DefSubclassProc`/`TaskDialogIndirect` by name — all of
/// which `tao`, `wry`, `tauri-runtime-wry` and `muda` import. The exe never gets
/// as far as `main`: the loader kills it with `0xc0000139
/// STATUS_ENTRYPOINT_NOT_FOUND`, before a single test runs.
///
/// Cargo only offers `rustc-link-arg-tests` for this (a lib's own `#[cfg(test)]`
/// unit-test executable never receives it — verified with a toy crate), which is
/// why the tests live in `tests/`, not in `src/lib.rs`.
fn embed_common_controls_manifest_for_tests() {
    use std::{env, fs, path::PathBuf};

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest = manifest_dir.join("windows-test-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());

    // 1 = CREATEPROCESS_MANIFEST_RESOURCE_ID, 24 = RT_MANIFEST. Forward slashes:
    // backslashes are escapes inside an .rc string literal.
    let rc = PathBuf::from(env::var("OUT_DIR").unwrap()).join("test-manifest.rc");
    let manifest_path = manifest.display().to_string().replace('\\', "/");
    fs::write(&rc, format!("1 24 \"{manifest_path}\"\n")).expect("write test-manifest.rc");

    let result = embed_resource::compile_for_tests(&rc, embed_resource::NONE);
    // On a real Windows host a silently skipped resource compile would just
    // reproduce the crash this exists to fix, so fail loudly there; when
    // cross-checking from another host there's no resource compiler to run.
    let checked = if cfg!(windows) { result.manifest_required() } else { result.manifest_optional() };
    if let Err(e) = checked {
        panic!("failed to embed the Common Controls v6 manifest for test executables: {e}");
    }
}
