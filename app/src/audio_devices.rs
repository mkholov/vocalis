//! Shared `cpal` device enumeration/selection: lists devices for the Settings
//! screen's dropdowns, and resolves a chosen device name back to an actual
//! `cpal::Device` for every capture/playback pipeline (mic, listen-in,
//! speaker output) that needs to open one. Centralized here so all four of
//! those pipelines (teacher mic/listen-in, student mic/output) fall back to
//! the system default the same way when the configured device is missing,
//! rather than each reimplementing (and potentially disagreeing on) that
//! fallback.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// Every input (microphone) device's name, in whatever order `cpal` reports
/// them, for the Settings screen's dropdown. Best-effort: a device whose name
/// can't be read is silently skipped rather than failing the whole list.
pub fn list_input_device_names() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// See [`list_input_device_names`].
pub fn list_output_device_names() -> Vec<String> {
    cpal::default_host()
        .output_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Resolves `name` to an actual input device, falling back to the system
/// default if `name` is `None` or no longer matches any connected device
/// (unplugged, renamed by its driver, etc. since the setting was saved) —
/// a stale setting should never be the reason capture fails to start.
pub fn resolve_input_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        match host.input_devices().ok().and_then(|mut devices| devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            Some(device) => return Ok(device),
            None => tracing::warn!("configured microphone '{name}' not found, falling back to the system default"),
        }
    }
    host.default_input_device().context("no default microphone found")
}

/// See [`resolve_input_device`].
pub fn resolve_output_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        match host.output_devices().ok().and_then(|mut devices| devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            Some(device) => return Ok(device),
            None => tracing::warn!("configured output device '{name}' not found, falling back to the system default"),
        }
    }
    host.default_output_device().context("no default output device found")
}

static CONFIGURED_OUTPUT_DEVICE: OnceLock<Option<String>> = OnceLock::new();

/// Configures which output device [`resolve_configured_output_device`] (and so
/// every playback pipeline that calls it — teacher listen-in, student speaker
/// output) resolves to from here on. Meant to be called exactly once, early
/// at launch right after loading `Settings` — matches
/// `default_output_sample_rate`'s existing "resolve once at startup, use for
/// the process's whole lifetime" shape, since the output stream itself is
/// also only ever lazily started once per process. A later setting change
/// takes effect the next time the app starts, not by hot-swapping a stream
/// that may already be running.
pub fn configure_output_device(name: Option<String>) {
    let _ = CONFIGURED_OUTPUT_DEVICE.set(name);
}

/// See [`configure_output_device`] and [`resolve_output_device`] (which this
/// falls back to the system default through, the same as every other
/// pipeline, if `configure_output_device` was never called or was called
/// with `None`).
pub fn resolve_configured_output_device() -> Result<cpal::Device> {
    let name = CONFIGURED_OUTPUT_DEVICE.get().cloned().flatten();
    resolve_output_device(name.as_deref())
}
