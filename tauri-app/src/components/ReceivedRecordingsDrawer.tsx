import { AnimatePresence, motion } from "framer-motion";
import { Play, RefreshCw, Square, X } from "lucide-react";
import { useReceivedRecordings } from "../lib/useReceivedRecordings";

/** Step 7.5: voice recordings students have sent for real ("Отправить учителю" on the student console) —
 * a real `ClientToServer::FileOffer`, saved to disk by `teacher::net`'s unchanged handler and read back
 * here (`commands/teacher_session.rs`'s `list_received_recordings`/`read_received_recording`). Which
 * student and which reference a recording was compared against travel baked into its own file name
 * (`"<student> — <recording> (эталон: <title>)"` — see `student_recording.rs`'s `send_recording_to_teacher`
 * doc comment), so this stays a plain list rather than needing its own per-student/per-material lookup.
 * Overlays as a right-hand drawer, reachable from any tab, the same pattern `ChatDrawer` uses. */
export function ReceivedRecordingsDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const voice = useReceivedRecordings(open);

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="fixed inset-0 z-40 bg-black/50"
          />
          <motion.div
            initial={{ x: "100%" }}
            animate={{ x: 0 }}
            exit={{ x: "100%" }}
            transition={{ type: "spring", stiffness: 320, damping: 34 }}
            className="fixed right-0 top-0 z-50 flex h-full w-full max-w-sm flex-col border-l border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-5"
          >
            <div className="mb-4 flex items-center justify-between">
              <h2 className="text-lg font-semibold">Записи учеников</h2>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => voice.refresh()}
                  className="inline-flex items-center justify-center rounded-lg p-1.5 text-[var(--color-text-muted)] hover:bg-overlay hover:text-[var(--color-text-primary)]"
                  title="Обновить"
                >
                  <RefreshCw size={16} />
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  className="inline-flex items-center justify-center rounded-lg p-1.5 text-[var(--color-text-muted)] hover:bg-overlay hover:text-[var(--color-text-primary)]"
                >
                  <X size={18} />
                </button>
              </div>
            </div>

            {voice.error && <p className="mb-3 rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-danger-text">{voice.error}</p>}

            <div className="flex-1 overflow-y-auto pr-1">
              <AnimatePresence initial={false}>
                {voice.recordings.map((r) => (
                  <motion.div
                    key={r.name}
                    layout
                    initial={{ opacity: 0, y: 8 }}
                    animate={{ opacity: 1, y: 0 }}
                    className="mb-2 flex items-center gap-3 rounded-xl bg-overlay px-3 py-2"
                  >
                    <motion.button
                      type="button"
                      whileTap={{ scale: 0.9 }}
                      onClick={() => voice.togglePlay(r.name)}
                      className={
                        "flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-sm transition-colors " +
                        (voice.playing === r.name ? "bg-violet-400/30 text-accent-text" : "bg-overlay-hover text-[var(--color-text-muted)] hover:bg-overlay-strong")
                      }
                      title={voice.playing === r.name ? "Остановить" : "Прослушать"}
                    >
                      {voice.playing === r.name ? <Square size={13} /> : <Play size={13} />}
                    </motion.button>
                    <div className="min-w-0 flex-1 text-sm">{r.name.replace(/\.wav$/, "")}</div>
                  </motion.div>
                ))}
              </AnimatePresence>
            </div>

            {voice.recordings.length === 0 && (
              <p className="mt-3 text-sm text-[var(--color-text-muted)]">Пока никто не прислал запись.</p>
            )}
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
