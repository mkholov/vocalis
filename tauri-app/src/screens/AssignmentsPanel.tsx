import { useState, type Dispatch, type SetStateAction } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check, ClipboardList, Send, Users } from "lucide-react";
import { Panel } from "../components/ui/Panel";
import { Button } from "../components/ui/Button";
import { EmptyState } from "../components/ui/EmptyState";
import { AssignmentEditor } from "../components/AssignmentEditor";
import { sendAssignment } from "../lib/commands";
import { KIND_META, summarize, tint, type AssignmentContent, type AssignmentDraft, type AssignmentTemplate } from "../lib/assignments";

/** The little a target picker needs to know about a real connected student — same shape `ChatDrawer`'s
 * own minimal `ChatStudent` uses, for the same reason (structurally compatible with `LiveStudent` without
 * depending on the whole `useLiveClassroom` module). */
export interface AssignmentTarget {
  realId: string;
  name: string;
}

interface Props {
  templates: AssignmentTemplate[];
  onAdd: (title: string, content: AssignmentContent) => void;
  draft: AssignmentDraft;
  setDraft: Dispatch<SetStateAction<AssignmentDraft>>;
  /** Real connected students (`TeacherConsole`'s own `useLiveClassroom`) — who "Отправить выбранным" can
   * target. Empty selection below means "весь класс" (the same convention `playMaterial`'s "▶ Всем"
   * already uses), not "nobody". */
  students: AssignmentTarget[];
}

type SendStatus = { state: "sending" } | { state: "sent" } | { state: "error"; message: string };

/** Assignments screen — library + editor. Sending is real: `sendAssignment` persists a real `assignments`
 * row per targeted student and delivers a real `ServerToClient::AssignmentOffer` (`commands/
 * teacher_session.rs`'s `send_assignment`, reusing `TeacherApp::send_assignment_template`'s own steps).
 * The library itself (`templates`) still lives only in `TeacherConsole`'s memory, not the DB — a template
 * can be sent any number of times, but isn't itself a saved, reusable row across restarts yet. */
export function AssignmentsPanel({ templates, onAdd, draft, setDraft, students }: Props) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [status, setStatus] = useState<Record<number, SendStatus>>({});

  function toggleTarget(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function send(t: AssignmentTemplate) {
    if (status[t.id]?.state === "sending") return;
    setStatus((prev) => ({ ...prev, [t.id]: { state: "sending" } }));
    sendAssignment(t.title, t.content, Array.from(selected))
      .then(() => {
        setStatus((prev) => ({ ...prev, [t.id]: { state: "sent" } }));
        setTimeout(
          () =>
            setStatus((prev) => {
              if (prev[t.id]?.state !== "sent") return prev;
              const { [t.id]: _dropped, ...rest } = prev;
              return rest;
            }),
          1800,
        );
      })
      .catch((err) => setStatus((prev) => ({ ...prev, [t.id]: { state: "error", message: String(err) } })));
  }

  const targetLabel = selected.size === 0 ? "Всему классу" : `Выбранным (${selected.size})`;

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Задания</h1>
        <p className="text-sm text-[var(--color-text-muted)]">
          «Отправить» — реально, по сети: подключённые ученики получают задание сразу. Сама библиотека пока хранится только до
          закрытия приложения.
        </p>
      </div>

      <Panel>
        <div className="mb-3 flex items-center gap-2 text-sm font-medium text-[var(--color-text-muted)]">
          <Users size={15} />
          Кому отправлять — {targetLabel.toLowerCase()}
        </div>
        {students.length === 0 ? (
          <p className="text-sm text-[var(--color-text-muted)]">Никто не подключён — задание уйдёт всем, кто подключится позже.</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {students.map((s) => (
              <label
                key={s.realId}
                className={
                  "flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-1.5 text-sm transition-colors " +
                  (selected.has(s.realId)
                    ? "border-violet-400 bg-violet-400/10 text-[var(--color-text-primary)]"
                    : "border-[var(--color-border-subtle)] text-[var(--color-text-muted)] hover:bg-overlay")
                }
              >
                <input type="checkbox" checked={selected.has(s.realId)} onChange={() => toggleTarget(s.realId)} className="accent-violet-400" />
                {s.name}
              </label>
            ))}
          </div>
        )}
      </Panel>

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
                const st = status[t.id];
                return (
                  <motion.li
                    key={t.id}
                    layout
                    initial={{ opacity: 0, y: 8 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, height: 0 }}
                    className="flex flex-col gap-1.5 rounded-xl border border-[var(--color-border-subtle)] px-4 py-3"
                  >
                    <div className="flex items-center justify-between gap-3">
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
                      <Button
                        variant="secondary"
                        className="shrink-0 px-3 py-1.5 text-sm"
                        disabled={st?.state === "sending"}
                        onClick={() => send(t)}
                      >
                        {st?.state === "sent" ? <Check size={15} /> : <Send size={15} />}
                        {st?.state === "sending" ? "Отправка…" : st?.state === "sent" ? "Отправлено" : "Отправить"}
                      </Button>
                    </div>
                    {st?.state === "error" && <p className="text-xs text-danger-text">{st.message}</p>}
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
