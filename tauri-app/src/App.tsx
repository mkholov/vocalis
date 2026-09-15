import { useState } from "react";
import { AnimatePresence } from "framer-motion";
import { RolePicker } from "./screens/RolePicker";
import { TeacherFlow } from "./screens/TeacherFlow";
import { StudentFlow } from "./screens/StudentFlow";
import { TeacherClassGrid } from "./screens/TeacherClassGrid";

type Role = "picker" | "teacher" | "student" | "teacherGrid";

// Steps 3-4 of the Tauri migration (vocalis_roadmap.md, section 8): auth/
// connect for both roles (step 3), then the teacher's class grid (step 4)
// once "Начать урок" is confirmed. `list_classes` and `discover_teachers`
// (screens/TeacherFlow.tsx, screens/StudentFlow.tsx) are real step-2
// commands; the class grid itself is still a local mock (no network, no real
// audio/video — see screens/TeacherClassGrid.tsx) until a later step.
function App() {
  const [role, setRole] = useState<Role>("picker");
  const [className, setClassName] = useState("");

  if (role === "teacherGrid") {
    return <TeacherClassGrid className={className} onEnd={() => setRole("picker")} />;
  }

  return (
    <main className="relative flex h-screen w-screen items-center justify-center overflow-hidden bg-[var(--color-app)] p-6">
      {/* Ambient accent glow behind the card — purely decorative. */}
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_50%_35%,rgba(167,139,250,0.12),transparent_60%)]" />

      <AnimatePresence mode="wait">
        {role === "picker" && <RolePicker key="picker" onPick={setRole} />}
        {role === "teacher" && (
          <TeacherFlow
            key="teacher"
            onBack={() => setRole("picker")}
            onStart={(name) => {
              setClassName(name);
              setRole("teacherGrid");
            }}
          />
        )}
        {role === "student" && <StudentFlow key="student" onBack={() => setRole("picker")} />}
      </AnimatePresence>
    </main>
  );
}

export default App;
