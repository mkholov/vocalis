import { AnimatePresence, motion } from "framer-motion";
import { Download, Sparkles, X } from "lucide-react";
import { useUpdater } from "../lib/useUpdater";

/**
 * Auto-update notice — mounted once at the top of `App.tsx`, above whichever screen (role picker,
 * teacher/student console) happens to be showing, so it's visible no matter where a real update lands.
 * Never appears unless `useUpdater`'s real startup `check()` actually found a newer release; never
 * downloads or installs anything on its own — every state transition past "Доступна новая версия" needs an
 * explicit click on "Установить", exactly what "не тихо и не принудительно" asked for.
 */
export function UpdateBanner() {
  const { stage, update, progress, error, install, dismiss } = useUpdater();

  const percent = progress.totalBytes ? Math.min(100, Math.round((progress.downloadedBytes / progress.totalBytes) * 100)) : null;

  return (
    <AnimatePresence>
      {stage !== "idle" && update && (
        <motion.div
          initial={{ y: -60, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: -60, opacity: 0 }}
          transition={{ type: "spring", stiffness: 340, damping: 32 }}
          className="fixed left-1/2 top-3 z-[70] w-full max-w-xl -translate-x-1/2 px-4"
        >
          <div className="flex items-start gap-3 rounded-2xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] px-4 py-3 shadow-lg shadow-black/20">
            <div className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-violet-400/20 text-accent-text">
              <Sparkles size={16} />
            </div>

            <div className="min-w-0 flex-1">
              {stage === "available" && (
                <>
                  <div className="text-sm font-medium">
                    Доступно обновление Vocalis {update.version}
                    <span className="ml-1.5 font-normal text-[var(--color-text-muted)]">(сейчас {update.currentVersion})</span>
                  </div>
                  {update.body && <p className="mt-1 line-clamp-3 text-xs text-[var(--color-text-muted)]">{update.body}</p>}
                  <div className="mt-2 flex items-center gap-2">
                    <button
                      type="button"
                      onClick={install}
                      className="inline-flex items-center gap-1.5 rounded-lg bg-violet-500 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-violet-400"
                    >
                      <Download size={13} />
                      Установить и перезапустить
                    </button>
                    <button
                      type="button"
                      onClick={dismiss}
                      className="rounded-lg px-3 py-1.5 text-xs text-[var(--color-text-muted)] transition-colors hover:bg-overlay-hover hover:text-[var(--color-text-primary)]"
                    >
                      Позже
                    </button>
                  </div>
                </>
              )}

              {stage === "downloading" && (
                <>
                  <div className="text-sm font-medium">Устанавливаю обновление {update.version}…</div>
                  <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-overlay">
                    <motion.div
                      className="h-full rounded-full bg-violet-400"
                      animate={{ width: percent !== null ? `${percent}%` : ["10%", "90%", "10%"] }}
                      transition={percent !== null ? { duration: 0.2 } : { duration: 1.6, repeat: Infinity, ease: "easeInOut" }}
                    />
                  </div>
                  <p className="mt-1 text-xs text-[var(--color-text-muted)]">
                    {percent !== null ? `${percent}%` : "Скачивание…"} — приложение перезапустится само после установки.
                  </p>
                </>
              )}

              {stage === "error" && (
                <>
                  <div className="text-sm font-medium text-danger-text">Не удалось установить обновление</div>
                  <p className="mt-1 text-xs text-[var(--color-text-muted)]">{error}</p>
                  <div className="mt-2 flex items-center gap-2">
                    <button
                      type="button"
                      onClick={install}
                      className="rounded-lg bg-violet-500 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-violet-400"
                    >
                      Повторить
                    </button>
                    <button
                      type="button"
                      onClick={dismiss}
                      className="rounded-lg px-3 py-1.5 text-xs text-[var(--color-text-muted)] transition-colors hover:bg-overlay-hover hover:text-[var(--color-text-primary)]"
                    >
                      Закрыть
                    </button>
                  </div>
                </>
              )}
            </div>

            {stage === "available" && (
              <button
                type="button"
                onClick={dismiss}
                className="shrink-0 rounded-lg p-1 text-[var(--color-text-muted)] hover:bg-overlay-hover hover:text-[var(--color-text-primary)]"
                title="Закрыть"
              >
                <X size={15} />
              </button>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
