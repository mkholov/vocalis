import { useState } from "react";
import { motion } from "framer-motion";
import { StudentCard } from "../components/StudentCard";
import { LessonTimer } from "../components/LessonTimer";
import { Button } from "../components/ui/Button";
import { useMockClassroom } from "../lib/mockClassroom";

interface Props {
  className: string;
  onEnd: () => void;
}

/** Step 4 of the Tauri migration (vocalis_roadmap.md, section 8): the
 * teacher's class grid. Everything a student card shows (presence, VU
 * level) is simulated locally (`useMockClassroom`) — no network, no real
 * audio/video yet (that's step 7). What *is* real here is the local UI
 * state: selecting a card, toggling its lock buttons, and the countdown
 * timer all actually work, they just don't talk to anyone yet. */
export function TeacherClassGrid({ className, onEnd }: Props) {
  const { students, toggleScreenLock, toggleMicLock } = useMockClassroom(12);
  const [selectedId, setSelectedId] = useState<number | null>(null);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-[var(--color-app)] p-6">
      <div className="pointer-events-none fixed inset-0 bg-[radial-gradient(circle_at_50%_0%,rgba(167,139,250,0.08),transparent_55%)]" />

      <motion.header
        initial={{ opacity: 0, y: -12 }}
        animate={{ opacity: 1, y: 0 }}
        className="relative z-10 mb-6 flex flex-wrap items-center justify-between gap-4"
      >
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{className}</h1>
          <p className="text-sm text-[var(--color-text-muted)]">
            {students.filter((s) => s.presence !== "empty").length} / {students.length} мест занято
          </p>
        </div>

        <LessonTimer />

        <Button variant="secondary" onClick={onEnd}>
          Завершить урок
        </Button>
      </motion.header>

      <motion.div
        className="relative z-10 grid flex-1 auto-rows-min grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4 overflow-y-auto pb-4"
        initial="hidden"
        animate="visible"
        variants={{ visible: { transition: { staggerChildren: 0.04 } } }}
      >
        {students.map((s) => (
          <StudentCard
            key={s.id}
            student={s}
            selected={selectedId === s.id}
            onSelect={() => setSelectedId((cur) => (cur === s.id ? null : s.id))}
            onToggleScreenLock={() => toggleScreenLock(s.id)}
            onToggleMicLock={() => toggleMicLock(s.id)}
          />
        ))}
      </motion.div>
    </div>
  );
}
