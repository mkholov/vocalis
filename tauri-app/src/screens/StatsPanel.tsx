import { motion } from "framer-motion";
import { Panel } from "../components/ui/Panel";
import { EmptyState } from "../components/ui/EmptyState";
import { ROSTER } from "../lib/mockClassroom";

interface StudentStat {
  name: string;
  score: number;
  done: number;
  total: number;
}

// Deterministic mock scores (not Math.random() — a stable, reproducible
// screen is easier to review than one that changes every reload). No stats
// Tauri command exists yet, and the roadmap explicitly said not to add one
// this step — this is UI only, wired to a real command whenever one exists.
const MOCK_STATS: StudentStat[] = ROSTER.map((name, i) => ({
  name,
  score: 60 + ((i * 37) % 40),
  done: 3 + (i % 4),
  total: 6,
}));

function summaryTiles(stats: StudentStat[]) {
  return [
    { label: "УРОКОВ ПРОВЕДЕНО", value: "12" },
    { label: "СРЕДНИЙ БАЛЛ", value: `${Math.round(stats.reduce((sum, s) => sum + s.score, 0) / stats.length)}%` },
    { label: "ЗАДАНИЙ ВЫПОЛНЕНО", value: String(stats.reduce((sum, s) => sum + s.done, 0)) },
    { label: "ПОСЕЩАЕМОСТЬ", value: "92%" },
  ];
}

/** `stats` is the seam for real data: empty (no lessons yet / nothing came back) shows the empty state
 * below; until a stats command exists it defaults to the deterministic sample above, which the screen
 * labels as sample data so it can't be mistaken for the real class. */
export function StatsPanel({ stats = MOCK_STATS }: { stats?: StudentStat[] }) {
  if (stats.length === 0) {
    return (
      <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Статистика</h1>
          <p className="text-sm text-[var(--color-text-muted)]">Сводка по урокам и ученикам.</p>
        </div>
        <Panel>
          <EmptyState
            icon="📊"
            title="Статистики пока нет"
            hint="Она появится после первого проведённого урока: посещаемость, выполненные задания и средний балл по каждому ученику."
          />
        </Panel>
      </div>
    );
  }

  const SUMMARY = summaryTiles(stats);
  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div>
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="text-2xl font-semibold tracking-tight">Статистика</h1>
          <span className="rounded-md bg-amber-400/15 px-2 py-0.5 text-xs font-medium text-amber-300">Пример данных</span>
        </div>
        <p className="text-sm text-[var(--color-text-muted)]">
          Это образец оформления, не ваш класс: реальная сводка появится вместе с командой, которая её считает.
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
          {stats.map((s) => (
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
