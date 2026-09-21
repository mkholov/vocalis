import { motion } from "framer-motion";
import { PinHero } from "./PinDisplay";

interface Props {
  pin: string | null;
  /** Set when the session itself could not start — then "waiting for students" would be a lie. */
  error?: string;
}

/** Shown above the (empty) seat grid while nobody is connected: the lesson PIN, large, and what to do
 * with it. Not shown once a real student is in. */
export function WaitingForStudents({ pin, error }: Props) {
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8, height: 0, marginBottom: 0, paddingTop: 0, paddingBottom: 0 }}
      transition={{ duration: 0.25 }}
      className="relative z-10 mb-6 flex flex-col items-center gap-4 overflow-hidden rounded-2xl border border-violet-400/25 bg-violet-400/[0.06] px-6 py-8 text-center"
    >
      {error ? (
        <p className="text-sm text-rose-400">Сессия не запущена — ученики не смогут подключиться.</p>
      ) : pin ? (
        <>
          <PinHero pin={pin} />
          <div className="flex items-center gap-2.5 text-lg font-medium">
            <motion.span
              className="h-2.5 w-2.5 rounded-full bg-violet-400"
              animate={{ opacity: [1, 0.25, 1], scale: [1, 0.8, 1] }}
              transition={{ repeat: Infinity, duration: 1.6, ease: "easeInOut" }}
            />
            Ждём подключения учеников — сообщите им PIN
          </div>
          <p className="max-w-md text-sm text-[var(--color-text-muted)]">
            Ученик открывает Vocalis, выбирает вас в списке преподавателей и вводит этот PIN.
          </p>
        </>
      ) : (
        <div className="flex items-center gap-2 text-sm text-[var(--color-text-muted)]">
          <motion.span
            className="h-3.5 w-3.5 rounded-full border-2 border-violet-400 border-t-transparent"
            animate={{ rotate: 360 }}
            transition={{ repeat: Infinity, duration: 0.7, ease: "linear" }}
          />
          Запускаю сессию…
        </div>
      )}
    </motion.div>
  );
}
