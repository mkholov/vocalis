import { useState, type Dispatch, type SetStateAction } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check, ClipboardList, Send } from "lucide-react";
import { Panel } from "../components/ui/Panel";
import { Button } from "../components/ui/Button";
import { EmptyState } from "../components/ui/EmptyState";
import { AssignmentEditor } from "../components/AssignmentEditor";
import { KIND_META, summarize, tint, type AssignmentContent, type AssignmentDraft, type AssignmentTemplate } from "../lib/assignments";


interface Props {
  templates: AssignmentTemplate[];
  onAdd: (title: string, content: AssignmentContent) => void;
  draft: AssignmentDraft;
  setDraft: Dispatch<SetStateAction<AssignmentDraft>>;
}

/** Assignments screen — library + editor. Templates and the draft live in `TeacherConsole` (so they
 * survive tab switches) and only in memory: there is no save/send command yet, so nothing persists across
 * restarts and "Отправить" only shows a transient confirmation rather than notifying anyone. The editor
 * builds the same structure as `common::protocol::AssignmentContent` (see `lib/assignments.ts`), so a
 * real command can take it as-is. */
export function AssignmentsPanel({ templates, onAdd, draft, setDraft }: Props) {
  const [justSentId, setJustSentId] = useState<number | null>(null);

  function markSent(id: number) {
    setJustSentId(id);
    setTimeout(() => setJustSentId((cur) => (cur === id ? null : cur)), 1600);
  }

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Задания</h1>
        <p className="text-sm text-[var(--color-text-muted)]">
          Отправка пока пробная: задания хранятся только до закрытия приложения и никому не уходят.
        </p>
      </div>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Библиотека заданий</h2>
        {templates.length === 0 ? (
          <EmptyState
            icon={<ClipboardList />}
            title="Пока нет ни одного задания"
            hint="Соберите первое в форме «Создать задание» ниже — оно появится здесь."
          />
        ) : (
          <ul className="flex flex-col gap-2">
            <AnimatePresence initial={false}>
              {templates.map((t) => {
                const meta = KIND_META[t.content.kind];
                return (
                  <motion.li
                    key={t.id}
                    layout
                    initial={{ opacity: 0, y: 8 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, height: 0 }}
                    className="flex items-center justify-between gap-3 rounded-xl border border-[var(--color-border-subtle)] px-4 py-3"
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <span
                        className="shrink-0 rounded-md px-2 py-0.5 text-xs font-medium"
                        style={{ backgroundColor: tint(meta.color), color: meta.color }}
                      >
                        {meta.label}
                      </span>
                      <div className="min-w-0">
                        <div className="truncate">{t.title}</div>
                        <div className="truncate text-xs text-[var(--color-text-muted)]">{summarize(t.content)}</div>
                      </div>
                    </div>
                    <Button variant="secondary" className="shrink-0 px-3 py-1.5 text-sm" onClick={() => markSent(t.id)}>
                      {justSentId === t.id ? <Check size={15} /> : <Send size={15} />}
                      {justSentId === t.id ? "Отправлено" : "Отправить"}
                    </Button>
                  </motion.li>
                );
              })}
            </AnimatePresence>
          </ul>
        )}
      </Panel>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Создать задание</h2>
        <AssignmentEditor draft={draft} setDraft={setDraft} onSave={onAdd} />
      </Panel>
    </div>
  );
}
