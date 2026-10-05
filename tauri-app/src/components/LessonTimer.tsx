import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Pause, Play, RotateCcw } from "lucide-react";
import { Button } from "./ui/Button";

const RADIUS = 30;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
// 1/5/15 for short activities (unchanged — a quick exercise or a listening clip is still just a few
// minutes), 45/60/90 for a real lesson: a standard class period, an explicit 60 as a boundary check (an
// hour is exactly where `formatTime` below switches from mm:ss to h:mm:ss — worth having as a one-tap
// preset, not just reachable by typing), and 90 for a сдвоенный урок (a double period).
const PRESETS_MIN = [1, 5, 15, 45, 60, 90];
const DEFAULT_MINUTES = 45;

function formatTime(totalSeconds: number) {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  // mm:ss under an hour (unchanged from before — "05:00" reads better than "0:05:00" for a short
  // activity), h:mm:ss from an hour up — "45:00" would still technically be unambiguous at 45 minutes,
  // but "90:00" for a double lesson reads like an error, not ninety minutes; "1:30:00" doesn't.
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Countdown timer for a lesson/assignment — start/pause/reset with a
 * settable duration, big and legible (per the roadmap: "крупно и заметно на
 * экране учителя"), with an animated ring showing progress. Purely local
 * state — nothing here is broadcast to students yet (that's a later step,
 * once there's a real session to broadcast it over). */
export function LessonTimer() {
  const [totalSeconds, setTotalSeconds] = useState(DEFAULT_MINUTES * 60);
  const [remaining, setRemaining] = useState(DEFAULT_MINUTES * 60);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) {
          clearInterval(id);
          setRunning(false);
          return 0;
        }
        return r - 1;
      });
    }, 1000);
    return () => clearInterval(id);
  }, [running]);

  const progress = totalSeconds > 0 ? remaining / totalSeconds : 0;
  const done = remaining === 0;
  const urgent = !done && remaining <= totalSeconds * 0.1;
  const ringColor = done ? "var(--color-status-danger)" : urgent ? "var(--color-status-warn)" : "var(--color-accent)"; // theme::DANGER / WARN / ACCENT
  // "45:00" fits the 64px dial fine at text-sm; "1:30:00" (the h:mm:ss form `formatTime` switches to past
  // an hour) is two characters longer and visibly overflows the ring at that size — confirmed by actually
  // rendering it, not just estimating width. Dropping to text-[11px] for that case is enough to fit it
  // cleanly without shrinking every other duration's (larger, more legible) label.
  const hasHourDigit = remaining >= 3600;

  function setPreset(minutes: number) {
    setRunning(false);
    setTotalSeconds(minutes * 60);
    setRemaining(minutes * 60);
  }

  function reset() {
    setRunning(false);
    setRemaining(totalSeconds);
  }

  return (
    <div className="flex items-center gap-5 rounded-2xl border border-[var(--color-border-subtle)] bg-[var(--color-card)] px-5 py-3 shadow-lg shadow-black/20 backdrop-blur-xl">
      <div className="relative h-16 w-16 shrink-0">
        <svg viewBox="0 0 70 70" className="h-16 w-16 -rotate-90">
          <circle cx="35" cy="35" r={RADIUS} fill="none" stroke="var(--color-overlay-hover)" strokeWidth="6" />
          <motion.circle
            cx="35"
            cy="35"
            r={RADIUS}
            fill="none"
            stroke={ringColor}
            strokeWidth="6"
            strokeLinecap="round"
            strokeDasharray={CIRCUMFERENCE}
            animate={{ strokeDashoffset: CIRCUMFERENCE * (1 - progress) }}
            transition={{ duration: 0.4, ease: "linear" }}
          />
        </svg>
        <div
          className={
            "absolute inset-0 flex items-center justify-center font-mono font-semibold tabular-nums " +
            (hasHourDigit ? "text-[11px]" : "text-sm")
          }
        >
          {formatTime(remaining)}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex gap-1">
          {PRESETS_MIN.map((m) => (
            <button
              key={m}
              type="button"
              disabled={running}
              onClick={() => setPreset(m)}
              className={
                "rounded-md px-2 py-0.5 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40 " +
                (totalSeconds === m * 60
                  ? "bg-violet-400/20 text-accent-text"
                  : "text-[var(--color-text-muted)] hover:bg-overlay")
              }
            >
              {m} мин
            </button>
          ))}
        </div>
        <div className="flex gap-2">
          <Button
            type="button"
            variant={running ? "secondary" : "primary"}
            className="px-4 py-1.5 text-sm"
            disabled={done}
            onClick={() => setRunning((r) => !r)}
          >
            {running ? <Pause size={15} /> : <Play size={15} />}
            {running ? "Пауза" : "Старт"}
          </Button>
          <Button type="button" variant="ghost" className="px-3 py-1.5 text-sm" onClick={reset}>
            <RotateCcw size={14} />
            Сброс
          </Button>
        </div>
      </div>
    </div>
  );
}
