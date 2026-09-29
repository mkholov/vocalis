import { useCallback, useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** How long the startup check is allowed to hang before giving up — the app's own core function (LAN
 * classroom, no internet needed) must never wait on this. Passed straight to the plugin's own `check()`
 * timeout (an actual bounded HTTP client timeout, not a client-side race — the plugin supports this
 * natively, see `CheckOptions.timeout` in `@tauri-apps/plugin-updater`), so a school with no internet, or a
 * LAN that can reach GitHub only slowly, never makes this hang the UI. */
const CHECK_TIMEOUT_MS = 6000;

export type UpdateStage =
  | "idle" // nothing to show — either no update, still checking, or the check failed/timed out (silently)
  | "available" // a real update was found; waiting on the teacher to decide
  | "downloading"
  | "error";

export interface UpdateDownloadProgress {
  downloadedBytes: number;
  /** `null` when the server didn't send a Content-Length — still shows a spinner, just no percentage. */
  totalBytes: number | null;
}

/**
 * Checks for a Vocalis update once, on mount (real startup check — see `App.tsx`), and exposes the whole
 * "install this update" flow, gated entirely on an explicit click: `check()` alone never downloads or
 * installs anything, and `install()` only runs when the caller (the update banner's own "Установить"
 * button) calls it — nothing here is silent or automatic, matching a school teacher's expectation of
 * knowing what's happening to their machine.
 *
 * The manifest and signed installer both come from GitHub Releases (`tauri.conf.json`'s
 * `plugins.updater.endpoints`, pointed at the repo's `.../releases/latest/download/latest.json` — see
 * `.github/workflows/tauri-windows-build.yml` for how that gets published on a version tag) — reusing the
 * GitHub Actions infrastructure this project already runs its CI on, rather than standing up and
 * maintaining a separate update server for a project this size.
 *
 * A network failure, timeout, or simply no update available are all treated the same way: silently back to
 * `"idle"`. There is deliberately no error banner for "couldn't check for updates" — a school PC with no
 * internet access (or a temporarily unreachable GitHub) is an entirely normal, expected state for this
 * app, not a problem to surface to a teacher who is about to start a lesson.
 */
export function useUpdater() {
  const [stage, setStage] = useState<UpdateStage>("idle");
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<UpdateDownloadProgress>({ downloadedBytes: 0, totalBytes: null });
  const [error, setError] = useState<string | undefined>();

  const checkNow = useCallback(() => {
    let cancelled = false;
    check({ timeout: CHECK_TIMEOUT_MS })
      .then((found) => {
        if (cancelled || !found) return;
        setUpdate(found);
        setStage("available");
      })
      .catch(() => {
        // See the doc comment above — never surfaced to the user.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // The real startup check — runs once, does not block rendering (React keeps rendering the app while this
  // promise is in flight; only this hook's own state updates once it resolves).
  useEffect(() => checkNow(), [checkNow]);

  const install = useCallback(async () => {
    if (!update) return;
    setStage("downloading");
    setProgress({ downloadedBytes: 0, totalBytes: null });
    setError(undefined);
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setProgress({ downloadedBytes: 0, totalBytes: event.data.contentLength ?? null });
        } else if (event.event === "Progress") {
          setProgress((p) => ({ downloadedBytes: p.downloadedBytes + event.data.chunkLength, totalBytes: p.totalBytes }));
        }
      });
      // Windows: `downloadAndInstall` above already exited the process once the installer launched (the
      // real NSIS installer this project ships, running with /P /UPDATE /R — passive UI, then relaunches
      // Vocalis itself once done) — this line is unreachable there. It's real and needed on macOS/Linux,
      // which don't auto-exit/relaunch on install, kept for a correct cross-platform dev build.
      await relaunch();
    } catch (err) {
      setStage("error");
      setError(String(err));
    }
  }, [update]);

  const dismiss = useCallback(() => {
    setStage("idle");
  }, []);

  return { stage, update, progress, error, install, dismiss, checkNow };
}
