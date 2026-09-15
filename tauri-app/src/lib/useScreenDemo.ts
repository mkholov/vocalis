import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { startScreenDemo, stopScreenDemo, type ScreenDemoFrameDto } from "./commands";

/** Real screen-demo video (step 7, part B — `vocalis_roadmap.md`, section 8):
 * while `active`, starts the real capture → H.264 encode → decode → JPEG
 * loop (`commands/screen_demo.rs`, reusing `vocalis::screen_capture`/
 * `vocalis::video` unchanged) and keeps `frame` updated with whatever this
 * machine's own screen actually looks like. This is a self-preview — there's
 * no class-wide network relay wired into the Tauri layer yet (see that
 * file's doc comment) — but the capture/codec/JPEG/IPC path is entirely
 * real, not mocked. `error` is set on a genuine failure (e.g. no accessible
 * monitor) rather than thrown.
 *
 * Mirrors `useMicMeter`'s start-on-effect/stop-on-cleanup shape, except
 * gated on `active` instead of always running — a screen-capture loop is far
 * heavier than a mic meter, so it should only run while actually shown. */
export function useScreenDemo(active: boolean) {
  const [frame, setFrame] = useState<ScreenDemoFrameDto | null>(null);
  const [error, setError] = useState<string | undefined>();

  useEffect(() => {
    if (!active) {
      setFrame(null);
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    startScreenDemo()
      .then(() => (cancelled ? undefined : listen<ScreenDemoFrameDto>("screen-demo-frame", (event) => setFrame(event.payload))))
      .then((fn) => {
        if (cancelled) fn?.();
        else unlisten = fn;
      })
      .catch((err) => setError(String(err)));

    return () => {
      cancelled = true;
      unlisten?.();
      setFrame(null);
      stopScreenDemo().catch(() => {});
    };
  }, [active]);

  return { frame, error };
}
