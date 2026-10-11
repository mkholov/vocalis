//! Read side of "режим кабинета" — see `device_role` (the crate-root module, not this one) for the whole
//! design and how the role actually gets set. Deliberately read-only from the frontend: nothing in the
//! running app's own UI should ever be able to pin or unpin a device's role, only `--set-role` run
//! elevated, outside the app, can.

#[tauri::command]
pub fn get_device_role() -> Option<String> {
    crate::device_role::read_role().map(|r| match r {
        crate::device_role::Role::Teacher => "teacher".to_string(),
        crate::device_role::Role::Student => "student".to_string(),
    })
}
