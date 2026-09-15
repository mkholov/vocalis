import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Button } from "./ui/Button";

const RADIUS = 30;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const PRESETS_MIN = [1, 5, 10, 15];

function formatTime(totalSeconds: number) {
  const m = Math.floor(totalSeconds / 60);
  const s = totalSeconds % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Countdown timer for a lesson/assignment — start/pause/reset with a
 * settable duration, big and legible (per the roadmap: "крупно и заметно на
 * экране учителя"), with an animated ring showing progress. Purely local
 * state — nothing here is broadcast to students yet (that's a later step,
 * once there's a real session to broadcast it over). */
export function LessonTimer() {
  const [totalSeconds, setTotalSeconds] = useState(5 * 60);
  const [remaining, setRemaining] = useState(5 * 60);
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
  const ringColor = done ? "#e05252" : urgent ? "#e6a84a" : "#a78bfa"; // theme::DANGER / WARN / ACCENT

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
          <circle cx="35" cy="35" r={RADIUS} fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth="6" />
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
        <div className="absolute inset-0 flex items-center justify-center font-mono text-sm font-semibold tabular-nums">
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
                  ? "bg-violet-400/20 text-violet-300"
                  : "text-[var(--color-text-muted)] hover:bg-white/5")
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
            {running ? "⏸ Пауза" : "▶ Старт"}
          </Button>
          <Button type="button" variant="ghost" className="px-3 py-1.5 text-sm" onClick={reset}>
            ↺ Сброс
          </Button>
        </div>
      </div>
    </div>
  );
}
