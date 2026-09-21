import { useState } from "react";
import { AnimatePresence } from "framer-motion";
import { RolePicker } from "./screens/RolePicker";
import { TeacherFlow } from "./screens/TeacherFlow";
import { StudentFlow } from "./screens/StudentFlow";
import { TeacherConsole } from "./screens/TeacherConsole";
import { StudentConsole } from "./screens/StudentConsole";
import type { DiscoveredTeacherDto } from "./lib/commands";

type Role = "picker" | "teacher" | "student" | "teacherConsole" | "studentConsole";

// Steps 3-7 of the Tauri migration (vocalis_roadmap.md, section 8): auth/
// connect for both roles (step 3), then the teacher console (steps 4-5) or
// student console (step 6) once the respective "start"/"connect" step is
// confirmed. `list_classes`/`discover_teachers`/`list_audio_devices` are real
// step-2 commands; `StudentFlow`'s "connect" step is a real
// `discover_teachers` pick too (step 7 part B) — the actual
// `connect_student_session` dial happens once `StudentConsole` mounts, using
// the teacher/pin this component threads through below.
function App() {
  const [role, setRole] = useState<Role>("picker");
  const [className, setClassName] = useState("");
  const [studentName, setStudentName] = useState("");
  const [connectedTeacher, setConnectedTeacher] = useState<DiscoveredTeacherDto | null>(null);
  const [pin, setPin] = useState("");

  if (role === "teacherConsole") {
    return <TeacherConsole className={className} onEnd={() => setRole("picker")} />;
  }
  if (role === "studentConsole" && connectedTeacher) {
    return (
      <StudentConsole
        studentName={studentName}
        teacherIp={connectedTeacher.ip}
        controlPort={connectedTeacher.controlPort}
        pin={pin}
        onDisconnect={() => setRole("picker")}
      />
    );
  }

  // The card hangs from a fixed height (`pt-[14vh]`) instead of being centred, so the heading stays put as a
  // flow moves between steps of different height; `overflow-y-auto` covers windows too short for a card.
  return (
        <main className="relative flex h-screen w-screen items-start justify-center overflow-y-auto bg-[var(--color-app)] px-6 pb-6 pt-[14vh]">
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
        {role === "student" && (
          <StudentFlow
            key="student"
            onBack={() => setRole("picker")}
            onConnect={(name, teacher, pinCode) => {
              setStudentName(name);
              setConnectedTeacher(teacher);
              setPin(pinCode);
              setRole("studentConsole");
            }}
          />
        )}
      </AnimatePresence>
    </main>
  );
}

export default App;
