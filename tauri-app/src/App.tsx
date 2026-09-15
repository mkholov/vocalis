import { useState } from "react";
import { AnimatePresence } from "framer-motion";
import { RolePicker } from "./screens/RolePicker";
import { TeacherFlow } from "./screens/TeacherFlow";
import { StudentFlow } from "./screens/StudentFlow";
import { TeacherConsole } from "./screens/TeacherConsole";

type Role = "picker" | "teacher" | "student" | "teacherConsole";

// Steps 3-5 of the Tauri migration (vocalis_roadmap.md, section 8): auth/
// connect for both roles (step 3), then the teacher console — class grid,
// assignments, stats, settings, chat (steps 4-5) — once "Начать урок" is
// confirmed. `list_classes`/`discover_teachers`/`list_audio_devices` are real
// step-2 commands; everything inside `TeacherConsole` past that point is
// still local mock state (no network, no real audio/video — see that file
// and its children) until a later step.
function App() {
  const [role, setRole] = useState<Role>("picker");
  const [className, setClassName] = useState("");

  if (role === "teacherConsole") {
    return <TeacherConsole className={className} onEnd={() => setRole("picker")} />;
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
              setRole("teacherConsole");
            }}
          />
        )}
        {role === "student" && <StudentFlow key="student" onBack={() => setRole("picker")} />}
      </AnimatePresence>
    </main>
  );
}

export default App;
