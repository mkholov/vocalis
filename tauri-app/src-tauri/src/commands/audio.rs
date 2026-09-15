//! Audio-device commands — thin wrappers around `vocalis::audio_devices`,
//! the same `cpal` enumeration the egui apps' Settings screens use. Real
//! hardware devices on whatever machine this runs on, not a stub list.

use serde::Serialize;
use vocalis::audio_devices;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevicesDto {
    pub input_devices: Vec<String>,
    pub output_devices: Vec<String>,
}

#[tauri::command]
pub fn list_audio_devices() -> AudioDevicesDto {
    AudioDevicesDto {
        input_devices: audio_devices::list_input_device_names(),
        output_devices: audio_devices::list_output_device_names(),
    }
}
