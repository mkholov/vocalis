import { motion } from "framer-motion";
import { GraduationCap, Presentation } from "lucide-react";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";

interface Props {
  onPick: (role: "teacher" | "student") => void;
}

/** First screen: matches the egui launcher's role choice (`lib.rs`'s
 * `VocalisApp::Launcher`) — same fork, new look. */
export function RolePicker({ onPick }: Props) {
  return (
    <Card className="text-center">
      <motion.h1
        initial={{ opacity: 0, y: -8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.05 }}
        className="mb-1 text-3xl font-semibold tracking-tight text-accent"
      >
        Vocalis
      </motion.h1>
      <motion.p
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.1 }}
        className="mb-8 text-[var(--color-text-muted)]"
      >
        Лингафонный кабинет
      </motion.p>

      <motion.div
        className="flex flex-col gap-3"
        initial="hidden"
        animate="visible"
        variants={{ visible: { transition: { staggerChildren: 0.08, delayChildren: 0.15 } } }}
      >
        <motion.div variants={{ hidden: { opacity: 0, y: 10 }, visible: { opacity: 1, y: 0 } }}>
          <Button variant="primary" className="w-full py-3.5 text-base" onClick={() => onPick("teacher")}>
            <Presentation size={20} />
            Я преподаватель
          </Button>
        </motion.div>
        <motion.div variants={{ hidden: { opacity: 0, y: 10 }, visible: { opacity: 1, y: 0 } }}>
          <Button variant="secondary" className="w-full py-3.5 text-base" onClick={() => onPick("student")}>
            <GraduationCap size={20} />
            Я ученик
          </Button>
        </motion.div>
      </motion.div>
    </Card>
  );
}
