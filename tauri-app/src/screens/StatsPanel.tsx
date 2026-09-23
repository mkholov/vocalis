import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { ChartColumn, RefreshCw } from "lucide-react";
import { Panel } from "../components/ui/Panel";
import { EmptyState } from "../components/ui/EmptyState";
import { Button } from "../components/ui/Button";
import { classStats, type ClassStatsDto } from "../lib/commands";
import { plural } from "../lib/plural";

const pct = (v: number | null) => (v === null ? "—" : `${Math.round(v)}%`);

function summaryTiles(stats: ClassStatsDto) {
  return [
    { label: "УРОКОВ ПРОВЕДЕНО", value: String(stats.lessons) },
    { label: "СРЕДНИЙ БАЛЛ", value: pct(stats.avgScore) },
    { label: "ЗАДАНИЙ ВЫПОЛНЕНО", value: String(stats.assignmentsDone) },
    { label: "УЧЕНИКОВ В РОСТЕРЕ", value: String(stats.rosterSize) },
  ];
}

/** Real class history (`commands/db.rs`'s `class_stats`, built on the same `db::load_history_summary`/
 * `db::normalize_name` the egui console's own "Статистика" tab and per-student history card use) — no
 * mock data. Reloads whenever `className` changes (switching classes between lessons) and on demand via
 * the refresh button, since this is a DB snapshot, not something a live event updates mid-lesson. */
export function StatsPanel({ className }: { className: string }) {
  const [stats, setStats] = useState<ClassStatsDto | null>(null);
  const [error, setError] = useState<string | undefined>();
  const [loading, setLoading] = useState(true);

  const load = useCallback(() => {
    setLoading(true);
    classStats(className)
      .then((s) => {
        setStats(s);
        setError(undefined);
      })
      .catch((err) => setError(String(err)))
      .finally(() => setLoading(false));
  }, [className]);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Статистика</h1>
          <p className="text-sm text-[var(--color-text-muted)]">
            Реальная история класса «{className}» — из базы данных, не пример.
          </p>
        </div>
        <Button variant="secondary" className="shrink-0 px-3 py-2 text-sm" disabled={loading} onClick={load}>
          <RefreshCw size={15} className={loading ? "animate-spin" : ""} />
          Обновить
        </Button>
      </div>

      {error && (
        <Panel>
          <EmptyState icon={<ChartColumn />} title="Не удалось загрузить статистику" hint={error} />
        </Panel>
      )}

      {loading && stats === null && !error && (
        <Panel>
          <div className="flex items-center gap-2 py-6 text-sm text-[var(--color-text-muted)]">
            <motion.span
              className="h-3.5 w-3.5 rounded-full border-2 border-violet-400 border-t-transparent"
              animate={{ rotate: 360 }}
              transition={{ repeat: Infinity, duration: 0.7, ease: "linear" }}
            />
            Загружаю статистику из базы данных…
          </div>
        </Panel>
      )}

      {stats && stats.lessons === 0 && stats.students.length === 0 && !error && (
        <Panel>
          <EmptyState
            icon={<ChartColumn />}
            title="Статистики пока нет"
            hint="Она появится после первого проведённого урока: средний балл, выполненные задания и история по каждому ученику."
          />
        </Panel>
      )}

      {stats && (stats.lessons > 0 || stats.students.length > 0) && (
        <>
          <motion.div
            className="flex flex-wrap gap-3"
            initial="hidden"
            animate="visible"
            variants={{ visible: { transition: { staggerChildren: 0.05 } } }}
          >
            {summaryTiles(stats).map((tile) => (
              <motion.div
                key={tile.label}
                variants={{ hidden: { opacity: 0, y: 10 }, visible: { opacity: 1, y: 0 } }}
                className="flex min-w-[150px] flex-1 flex-col rounded-xl border border-[var(--color-border-subtle)] bg-subtle px-4 py-3"
              >
                <div className="text-xs text-[var(--color-text-muted)]">{tile.label}</div>
                <div className="mt-auto pt-1 text-2xl font-semibold">{tile.value}</div>
              </motion.div>
            ))}
          </motion.div>

          <Panel>
            <h2 className="mb-2 text-sm font-medium text-[var(--color-text-muted)]">Прогресс по ученикам</h2>
            {stats.students.length === 0 ? (
              <p className="py-4 text-sm text-[var(--color-text-muted)]">
                Уроки проходили, но ни один ученик пока не подключался под именем с историей.
              </p>
            ) : (
              <motion.ul
                className="flex flex-col divide-y divide-[var(--color-border-subtle)]"
                initial="hidden"
                animate="visible"
                variants={{ visible: { transition: { staggerChildren: 0.04 } } }}
              >
                {stats.students.map((s) => (
                  <motion.li
                    key={s.name}
                    variants={{ hidden: { opacity: 0, x: -8 }, visible: { opacity: 1, x: 0 } }}
                    className="grid grid-cols-[1fr_9rem_5rem] items-center gap-4 py-3"
                  >
                    <span className="min-w-0 truncate">
                      {s.name}
                      <span className="ml-2 text-xs text-[var(--color-text-muted)]">
                        {plural(s.lessons, "урок", "урока", "уроков")}
                      </span>
                    </span>
                    <span className="text-right text-sm tabular-nums text-[var(--color-text-muted)]">
                      {s.assignmentsDone}/{s.assignmentsTotal} заданий
                    </span>
                    <span className="text-right font-mono font-medium tabular-nums">{pct(s.avgScore)}</span>
                  </motion.li>
                ))}
              </motion.ul>
            )}
          </Panel>
        </>
      )}
    </div>
  );
}
