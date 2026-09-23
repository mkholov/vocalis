import { useSyncExternalStore } from "react";

/** Which microphone the teacher's own broadcast/intercom/mic-test should open — chosen once in Settings,
 * shared by every screen that opens a real capture, and remembered between launches (same
 * `localStorage`-backed pattern as `lib/theme.ts`). `undefined` means "system default", which is also
 * what happens if the remembered name no longer matches a real device (unplugged headset, etc.) —
 * `audio_devices::resolve_input_device` on the Rust side already falls back to the default in that case,
 * this just keeps the picker honest about it not being a specific device you can no longer see selected. */

const STORAGE_KEY = "vocalis-mic-device";
const listeners = new Set<() => void>();

export function readSelectedMicDevice(): string | undefined {
  try {
    return localStorage.getItem(STORAGE_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}

export function setSelectedMicDevice(name: string | undefined) {
  try {
    if (name) localStorage.setItem(STORAGE_KEY, name);
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    // not remembered — the selection still applies for the rest of this run via the listeners below
  }
  listeners.forEach((l) => l());
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** Current selection, re-rendering when it changes (e.g. Settings updates it while this screen is open
 * elsewhere). Validity against the real device list (does this name still exist?) is the caller's job —
 * this module only stores a name. */
export function useSelectedMicDevice(): string | undefined {
  return useSyncExternalStore(subscribe, readSelectedMicDevice);
}
