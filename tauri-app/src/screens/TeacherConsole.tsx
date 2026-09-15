import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { TeacherClassGrid } from "./TeacherClassGrid";
import { AssignmentsPanel } from "./AssignmentsPanel";
import { StatsPanel } from "./StatsPanel";
import { SettingsPanel } from "./SettingsPanel";
import { ChatDrawer } from "../components/ChatDrawer";

interface Props {
  className: string;
  onEnd: () => void;
}

type Tab = "class" | "assignments" | "stats" | "settings";

const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: "class", label: "Класс", icon: "🏠" },
  { id: "assignments", label: "Задания", icon: "📝" },
  { id: "stats", label: "Статистика", icon: "📊" },
  { id: "settings", label: "Настройки", icon: "⚙" },
];

/** Step 5 of the Tauri migration (vocalis_roadmap.md, section 8): the shell
 * every teacher screen lives in from here on — a left nav (analogous to the
 * egui console's top tab bar) with an animated active-tab indicator, plus a
 * chat drawer reachable from any tab. `TeacherClassGrid` (step 4) is nested
 * here unmodified apart from one flagged layout fix (see that file) — this
 * component owns navigation, not the grid's own content. */
export function TeacherConsole({ className, onEnd }: Props) {
  const [tab, setTab] = useState<Tab>("class");
  const [chatOpen, setChatOpen] = useState(false);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[var(--color-app)]">
      <nav className="flex w-20 shrink-0 flex-col items-center gap-1 border-r border-[var(--color-border-subtle)] bg-black/20 py-6">
        <div className="mb-4 text-xl font-bold text-violet-400">V</div>

        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className="relative flex w-16 flex-col items-center gap-1 rounded-xl py-2.5 text-xs transition-colors"
          >
            {tab === t.id && (
              <motion.div
                layoutId="teacher-nav-pill"
                className="absolute inset-0 rounded-xl bg-violet-400/15"
                transition={{ type: "spring", stiffness: 400, damping: 32 }}
              />
            )}
            <span className="relative z-10 text-lg leading-none">{t.icon}</span>
            <span className={"relative z-10 " + (tab === t.id ? "font-medium text-violet-300" : "text-[var(--color-text-muted)]")}>
              {t.label}
            </span>
          </button>
        ))}

        <div className="mt-auto flex flex-col items-center gap-2 border-t border-[var(--color-border-subtle)] pt-4">
          <button
            type="button"
            onClick={() => setChatOpen(true)}
            className="flex w-16 flex-col items-center gap-1 rounded-xl py-2.5 text-xs text-[var(--color-text-muted)] transition-colors hover:bg-white/5 hover:text-[var(--color-text-primary)]"
          >
            <span className="text-lg leading-none">💬</span>
            <span>Чат</span>
          </button>
        </div>
      </nav>

      <div className="relative min-w-0 flex-1">
        <AnimatePresence mode="wait">
          <motion.div
            key={tab}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.15 }}
            className="h-full w-full"
          >
            {tab === "class" && <TeacherClassGrid className={className} onEnd={onEnd} />}
            {tab === "assignments" && <AssignmentsPanel />}
            {tab === "stats" && <StatsPanel />}
            {tab === "settings" && <SettingsPanel />}
          </motion.div>
        </AnimatePresence>
      </div>

      <ChatDrawer open={chatOpen} onClose={() => setChatOpen(false)} />
    </div>
  );
}
