import { motion } from "framer-motion";
import { Panel } from "../components/ui/Panel";
import { ROSTER } from "../lib/mockClassroom";

// Deterministic mock scores (not Math.random() — a stable, reproducible
// screen is easier to review than one that changes every reload). No stats
// Tauri command exists yet, and the roadmap explicitly said not to add one
// this step — this is UI only, wired to a real command whenever one exists.
const STUDENT_STATS = ROSTER.map((name, i) => ({
  name,
  score: 60 + ((i * 37) % 40),
  done: 3 + (i % 4),
  total: 6,
}));

const SUMMARY = [
  { label: "УРОКОВ ПРОВЕДЕНО", value: "12" },
  { label: "СРЕДНИЙ БАЛЛ", value: `${Math.round(STUDENT_STATS.reduce((sum, s) => sum + s.score, 0) / STUDENT_STATS.length)}%` },
  { label: "ЗАДАНИЙ ВЫПОЛНЕНО", value: String(STUDENT_STATS.reduce((sum, s) => sum + s.done, 0)) },
  { label: "ПОСЕЩАЕМОСТЬ", value: "92%" },
];

export function StatsPanel() {
  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Статистика</h1>
        <p className="text-sm text-[var(--color-text-muted)]">
          Пока моковые данные — реальная сводка появится вместе с командой, которая её считает.
        </p>
      </div>

      <motion.div
        className="flex flex-wrap gap-3"
        initial="hidden"
        animate="visible"
        variants={{ visible: { transition: { staggerChildren: 0.05 } } }}
      >
        {SUMMARY.map((tile) => (
          <motion.div
            key={tile.label}
            variants={{ hidden: { opacity: 0, y: 10 }, visible: { opacity: 1, y: 0 } }}
            className="flex min-w-[150px] flex-1 flex-col rounded-xl border border-[var(--color-border-subtle)] bg-black/10 px-4 py-3"
          >
            <div className="text-xs text-[var(--color-text-muted)]">{tile.label}</div>
            <div className="mt-auto pt-1 text-2xl font-semibold">{tile.value}</div>
          </motion.div>
        ))}
      </motion.div>

      <Panel>
        <h2 className="mb-2 text-sm font-medium text-[var(--color-text-muted)]">Прогресс по ученикам</h2>
        <motion.ul
          className="flex flex-col divide-y divide-[var(--color-border-subtle)]"
          initial="hidden"
          animate="visible"
          variants={{ visible: { transition: { staggerChildren: 0.04 } } }}
        >
          {STUDENT_STATS.map((s) => (
            <motion.li
              key={s.name}
              variants={{ hidden: { opacity: 0, x: -8 }, visible: { opacity: 1, x: 0 } }}
              className="grid grid-cols-[1fr_8rem_3.5rem] items-center gap-4 py-3"
            >
              <span className="truncate">{s.name}</span>
              <span className="text-right text-sm tabular-nums text-[var(--color-text-muted)]">
                {s.done}/{s.total} заданий
              </span>
              <span className="text-right font-mono font-medium tabular-nums">{s.score}%</span>
            </motion.li>
          ))}
        </motion.ul>
      </Panel>
    </div>
  );
}
