import { useState } from "react";
import { AnimatePresence } from "framer-motion";
import { RolePicker } from "./screens/RolePicker";
import { TeacherFlow } from "./screens/TeacherFlow";
import { StudentFlow } from "./screens/StudentFlow";

type Role = "picker" | "teacher" | "student";

// Step 3 of the Tauri migration (vocalis_roadmap.md, section 8): the first
// real screen on the new stack — auth/connect for both roles in one app,
// switching between them rather than duplicating the shared UI primitives
// (components/ui/) across two separate apps. `list_classes` and
// `discover_teachers` (screens/TeacherFlow.tsx, screens/StudentFlow.tsx) are
// the real step-2 commands; what happens after a successful pick is still a
// mocked console.log — starting a real lesson/session is a later step.
function App() {
  const [role, setRole] = useState<Role>("picker");

  return (
    <main className="relative flex h-screen w-screen items-center justify-center overflow-hidden bg-[var(--color-app)] p-6">
      {/* Ambient accent glow behind the card — purely decorative. */}
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_50%_35%,rgba(167,139,250,0.12),transparent_60%)]" />

      <AnimatePresence mode="wait">
        {role === "picker" && <RolePicker key="picker" onPick={setRole} />}
        {role === "teacher" && <TeacherFlow key="teacher" onBack={() => setRole("picker")} />}
        {role === "student" && <StudentFlow key="student" onBack={() => setRole("picker")} />}
      </AnimatePresence>
    </main>
  );
}

export default App;
