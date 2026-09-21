import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Mic, Square } from "lucide-react";
import { Button } from "./ui/Button";
import { useMicMeter } from "../lib/useMicMeter";

const SEGMENTS = 24;
/** Raw levels (`teacher::mic::rms_millis`, RMS in thousandths of full scale): quiet speech is ~50, loud
 * ~250+. A square-root curve gives the quiet end room, so ordinary speech lands mid-bar, not at the floor. */
const FULL_SCALE_LEVEL = 300;
/** How long a running test may stay silent before we suggest the mic itself is the problem. */
const SILENCE_HINT_AFTER_MS = 3000;

function litSegments(level: number) {
  return Math.round(Math.min(1, Math.sqrt(Math.max(0, level) / FULL_SCALE_LEVEL)) * SEGMENTS);
}

function segmentColor(i: number) {
  const f = i / SEGMENTS;
  return f < 0.62 ? "var(--color-status-ok)" : f < 0.87 ? "var(--color-status-warn)" : "var(--color-status-danger)";
}

/** Live level bar — same input as the student console's VU meter, drawn wide enough to judge a mic by. */
function LevelBar({ level }: { level: number }) {
  const lit = litSegments(level);
  return (
    <div className="flex h-6 flex-1 items-stretch gap-[3px]" role="meter" aria-label="Уровень микрофона" aria-valuemin={0} aria-valuemax={SEGMENTS} aria-valuenow={lit}>
      {Array.from({ length: SEGMENTS }, (_, i) => (
        <div
          key={i}
          className="flex-1 rounded-[3px] transition-[background-color,opacity] duration-75"
          style={{ backgroundColor: i < lit ? segmentColor(i) : "var(--color-overlay-hover)" }}
        />
      ))}
    </div>
  );
}

/** "Проверить микрофон": a real capture on the chosen input with a live level bar, without starting a
 * lesson. Stops on the second click, when the input is changed (it restarts on the new one), and on leaving
 * the screen (the hook's cleanup). */
export function MicTest({ deviceName }: { deviceName?: string }) {
  const [testing, setTesting] = useState(false);
  const { level, error } = useMicMeter({ active: testing, deviceName });

  // A failed start leaves nothing capturing — drop back to "not testing" so the button matches reality.
  useEffect(() => {
    if (error) setTesting(false);
  }, [error]);

  // Heard nothing since the test started (or since the input was switched)?
  const [heardSomething, setHeardSomething] = useState(false);
  const [silent, setSilent] = useState(false);
  useEffect(() => {
    if (level > 2) setHeardSomething(true);
  }, [level]);
  useEffect(() => {
    setHeardSomething(false);
    setSilent(false);
    if (!testing) return;
    const t = setTimeout(() => setSilent(true), SILENCE_HINT_AFTER_MS);
    return () => clearTimeout(t);
  }, [testing, deviceName]);

  return (
    <div className="mt-3">
      <div className="flex items-center gap-3">
        <Button
          type="button"
          variant="secondary"
          aria-pressed={testing}
          className={"shrink-0 px-3.5 py-2 text-sm " + (testing ? "!border-violet-400/50 !bg-violet-400/15 !text-accent-text" : "")}
          onClick={() => setTesting((t) => !t)}
        >
          {testing ? <Square size={15} /> : <Mic size={15} />}
          {testing ? "Остановить проверку" : "Проверить микрофон"}
        </Button>
        <AnimatePresence>
          {testing && (
            <motion.div initial={{ opacity: 0, scaleX: 0.9 }} animate={{ opacity: 1, scaleX: 1 }} exit={{ opacity: 0 }} className="flex min-w-0 flex-1 origin-left">
              <LevelBar level={level} />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <div className="mt-1.5 min-h-[1.25rem] text-xs" role="status">
        {error ? (
          <span className="text-danger-text">Не удалось открыть микрофон: {error}</span>
        ) : testing && silent && !heardSomething ? (
          <span className="text-warn-text">Сигнала нет. Проверьте, что выбран нужный микрофон и он не выключен в системе.</span>
        ) : testing ? (
          <span className="text-[var(--color-text-muted)]">Скажите что-нибудь — полоса должна двигаться.</span>
        ) : null}
      </div>
    </div>
  );
}
