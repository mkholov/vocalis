import { useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ChartColumn, ClipboardList, LayoutGrid, MessageSquare, Settings, type LucideIcon } from "lucide-react";
import logo from "../assets/vocalis-logo.png";
import { TeacherClassGrid } from "./TeacherClassGrid";
import { AssignmentsPanel } from "./AssignmentsPanel";
import { StatsPanel } from "./StatsPanel";
import { SettingsPanel } from "./SettingsPanel";
import { emptyDraft, type AssignmentDraft, type AssignmentTemplate } from "../lib/assignments";
import { Onboarding } from "../components/Onboarding";
import { hasSeenOnboarding, markOnboardingSeen } from "../lib/onboarding";
import { ChatDrawer } from "../components/ChatDrawer";
import { ToastStack, useToasts } from "../components/Toast";
import { useLiveClassroom } from "../lib/useLiveClassroom";
import { useClassroomToasts } from "../lib/useClassroomToasts";

interface Props {
  className: string;
  onEnd: () => void;
}

type Tab = "class" | "assignments" | "stats" | "settings";

const TABS: { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: "class", label: "Класс", icon: LayoutGrid },
  { id: "assignments", label: "Задания", icon: ClipboardList },
  { id: "stats", label: "Статистика", icon: ChartColumn },
  { id: "settings", label: "Настройки", icon: Settings },
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
  // First-run walkthrough: opens by itself the first time the console is entered, and from Settings on demand.
  const [onboardingOpen, setOnboardingOpen] = useState(() => !hasSeenOnboarding());
  const closeOnboarding = () => {
    markOnboardingSeen();
    setOnboardingOpen(false);
  };

  // The real teacher session lives here, not in the class grid: the grid only mounts on the "Класс" tab,
  // so a session owned by it would be stopped (PIN lost, students dropped) on every switch to another
  // tab — and a raised hand couldn't toast on those tabs.
  const live = useLiveClassroom(className);
  // Assignments live here for the same reason (the tab unmounts on every switch): in memory only, until
  // there's a real save command.
  const [templates, setTemplates] = useState<AssignmentTemplate[]>([]);
  const [assignmentDraft, setAssignmentDraft] = useState<AssignmentDraft>(() => emptyDraft());
  const nextTemplateId = useRef(1);
  const toasts = useToasts();
  useClassroomToasts(live.realStudents, toasts.push);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[var(--color-app)]">
      <nav className="flex w-20 shrink-0 flex-col items-center gap-1 border-r border-[var(--color-border-subtle)] bg-field py-6">
        <img src={logo} alt="Vocalis" width={44} height={44} className="mb-4 h-11 w-11 rounded-xl shadow-md shadow-black/30" draggable={false} />

        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className="relative flex w-[4.5rem] flex-col items-center gap-1 rounded-xl py-2.5 text-xs outline-none transition-colors focus-visible:ring-2 focus-visible:ring-violet-400"
          >
            {tab === t.id && (
              <motion.div
                layoutId="teacher-nav-pill"
                className="absolute inset-0 rounded-xl bg-violet-400/15"
                transition={{ type: "spring", stiffness: 400, damping: 32 }}
              />
            )}
            <t.icon className={"relative z-10 h-5 w-5 " + (tab === t.id ? "text-accent-text" : "text-[var(--color-text-muted)]")} strokeWidth={tab === t.id ? 2.25 : 1.75} />
            <span className={"relative z-10 " + (tab === t.id ? "font-medium text-accent-text" : "text-[var(--color-text-muted)]")}>
              {t.label}
            </span>
          </button>
        ))}

        <div className="mt-auto flex flex-col items-center gap-2 border-t border-[var(--color-border-subtle)] pt-4">
          <button
            type="button"
            onClick={() => setChatOpen(true)}
            className="flex w-[4.5rem] flex-col items-center gap-1 rounded-xl py-2.5 text-xs text-[var(--color-text-muted)] outline-none transition-colors hover:bg-overlay hover:text-[var(--color-text-primary)] focus-visible:ring-2 focus-visible:ring-violet-400"
          >
            <MessageSquare className="h-5 w-5" strokeWidth={1.75} />
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
            {tab === "class" && <TeacherClassGrid className={className} live={live} onEnd={onEnd} />}
            {tab === "assignments" && (
              <AssignmentsPanel
                templates={templates}
                onAdd={(title, content) => setTemplates((prev) => [{ id: nextTemplateId.current++, title, content }, ...prev])}
                draft={assignmentDraft}
                setDraft={setAssignmentDraft}
              />
            )}
            {tab === "stats" && <StatsPanel />}
            {tab === "settings" && <SettingsPanel onShowOnboarding={() => setOnboardingOpen(true)} />}
          </motion.div>
        </AnimatePresence>
      </div>

      <ChatDrawer open={chatOpen} onClose={() => setChatOpen(false)} />
      <Onboarding open={onboardingOpen} onClose={closeOnboarding} />
      <ToastStack items={toasts.items} onDismiss={toasts.dismiss} />
    </div>
  );
}
