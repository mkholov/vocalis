//! Regression test for the Windows-only crash tracked upstream as RustAudio/cpal#1302
//! ("WASAPI: cached IMMDeviceEnumerator is created in one thread's STA and used after
//! that thread exits").
//!
//! cpal's WASAPI backend keeps a process-wide `IMMDeviceEnumerator`, created on
//! whichever thread touches cpal *first*, inside that thread's COM apartment (STA) —
//! and tears that apartment down (`CoUninitialize`) when that thread exits. Any later
//! enumeration from another thread then reads a dead vtable and the process dies with
//! `STATUS_ACCESS_VIOLATION (0xc0000005)` instead of returning an error or an empty
//! list — regardless of whether the machine has any audio devices at all. It shows up
//! as "passes alone, crashes when other tests ran first" (a libtest thread that ran an
//! earlier test was the first cpal user and has since exited), and equally in the real
//! app whenever the first cpal call happens on a short-lived thread (e.g. the student
//! mic meter's dedicated thread).
//!
//! This test lives alone in its own file so it gets a fresh process — cpal's global is
//! per-process, and any other test touching cpal first would change what it exercises.
//! On non-Windows it passes trivially; its job is to fail loudly on the Windows CI
//! runner if `vocalis::audio_devices` ever stops keeping cpal's first use on a
//! thread that lives for the whole process.

use vocalis::audio_devices;

#[test]
fn enumerating_devices_after_the_first_cpal_thread_exited_does_not_crash() {
    // 1. The first thread in this process to touch cpal, which then exits.
    std::thread::spawn(|| {
        let _ = audio_devices::list_input_device_names();
    })
    .join()
    .expect("first thread should finish");

    // 2. A different thread uses cpal afterwards — this is what dereferenced the
    //    dead enumerator. Every entry point `vocalis` exposes is exercised; an
    //    `Err` from `resolve_*` (no default device on this machine) is fine, a
    //    crash is not.
    std::thread::spawn(|| {
        let _ = audio_devices::list_input_device_names();
        let _ = audio_devices::list_output_device_names();
        let _ = audio_devices::resolve_input_device(None);
        let _ = audio_devices::resolve_output_device(None);
    })
    .join()
    .expect("second thread should finish");
}
