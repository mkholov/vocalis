// Assignment shapes and the pure logic around them — no React, no IPC.
//
// Two layers on purpose:
//  * `AssignmentDraft` — what the editor form edits. Every question/option has a stable `id`, so deleting
//    or adding rows never shifts which input is which, and the right answer is remembered by option id
//    (it survives deleting an option above it).
//  * `AssignmentContent` — what a saved assignment *is*, shaped like `common::protocol::AssignmentContent`
//    (`Test { questions: [{ text, options, correct_index }] }`, `Listening { material_title, questions }`,
//    `Reading { text }`) in camelCase. When there's a real save/send command, this is what it takes;
//    `draftToContent` is the only place that converts.

export type AssignmentKind = "test" | "listening" | "reading";

export interface TestQuestion {
  text: string;
  options: string[];
  correctIndex: number;
}

export type AssignmentContent =
  | { kind: "test"; questions: TestQuestion[] }
  | { kind: "listening"; materialTitle: string; questions: string[] }
  | { kind: "reading"; text: string };

export interface AssignmentTemplate {
  id: number;
  title: string;
  content: AssignmentContent;
}

export interface DraftOption {
  id: number;
  text: string;
}

export interface DraftTestQuestion {
  id: number;
  text: string;
  options: DraftOption[];
  /** `DraftOption.id` of the right answer, not an index. */
  correctOptionId: number;
}

export interface DraftPrompt {
  id: number;
  text: string;
}

export interface AssignmentDraft {
  kind: AssignmentKind;
  title: string;
  // One field group per kind, all kept while the teacher flips between kinds (switching tabs must not
  // throw away a half-written test).
  testQuestions: DraftTestQuestion[];
  listeningMaterial: string;
  listeningPrompts: DraftPrompt[];
  readingText: string;
}

export const MIN_OPTIONS = 2;

let nextLocalId = 1;
/** Local row key — never sent anywhere. */
const localId = () => nextLocalId++;

export function newQuestion(): DraftTestQuestion {
  const options = Array.from({ length: MIN_OPTIONS }, () => ({ id: localId(), text: "" }));
  return { id: localId(), text: "", options, correctOptionId: options[0].id };
}

export function newOption(): DraftOption {
  return { id: localId(), text: "" };
}

export function newPrompt(): DraftPrompt {
  return { id: localId(), text: "" };
}

export function emptyDraft(kind: AssignmentKind = "test"): AssignmentDraft {
  return {
    kind,
    title: "",
    testQuestions: [newQuestion()],
    listeningMaterial: "",
    listeningPrompts: [],
    readingText: "",
  };
}

/** Removes an option, keeping the right answer valid: if the removed one was marked correct, the first
 * remaining option becomes correct. Never drops below `MIN_OPTIONS`. */
export function removeOption(q: DraftTestQuestion, optionId: number): DraftTestQuestion {
  if (q.options.length <= MIN_OPTIONS) return q;
  const options = q.options.filter((o) => o.id !== optionId);
  const correctOptionId = q.correctOptionId === optionId ? options[0].id : q.correctOptionId;
  return { ...q, options, correctOptionId };
}

export interface DraftErrors {
  title?: string;
  /** Kind-level problem: no questions at all / no material / no text. */
  content?: string;
  /** Problems per test question, keyed by `DraftTestQuestion.id`. */
  questions: Map<number, string>;
}

const filled = (s: string) => s.trim().length > 0;

/** Everything wrong with the draft, in the words to show the teacher. Empty (`isValid`) means
 * `draftToContent` will succeed. The egui editor silently ignored a save it didn't like; this says why. */
export function validateDraft(d: AssignmentDraft): DraftErrors {
  const errors: DraftErrors = { questions: new Map() };
  if (!filled(d.title)) errors.title = "Введите название задания";

  if (d.kind === "test") {
    if (d.testQuestions.length === 0) errors.content = "Добавьте хотя бы один вопрос";
    for (const q of d.testQuestions) {
      const problem = !filled(q.text)
        ? "Введите текст вопроса"
        : q.options.filter((o) => filled(o.text)).length < MIN_OPTIONS
          ? `Заполните минимум ${MIN_OPTIONS} варианта ответа`
          : !filled(q.options.find((o) => o.id === q.correctOptionId)?.text ?? "")
            ? "Отмеченный правильным вариант пуст — отметьте заполненный"
            : undefined;
      if (problem) errors.questions.set(q.id, problem);
    }
  } else if (d.kind === "listening") {
    if (!filled(d.listeningMaterial)) errors.content = "Укажите материал для прослушивания";
  } else if (!filled(d.readingText)) {
    errors.content = "Введите текст для чтения";
  }
  return errors;
}

export function isValid(errors: DraftErrors): boolean {
  return !errors.title && !errors.content && errors.questions.size === 0;
}

/** Draft → saved shape. Call only for a draft that passed `validateDraft`. Blank options are dropped
 * (the right answer's index is recomputed among what's left); blank listening prompts are dropped. */
export function draftToContent(d: AssignmentDraft): AssignmentContent {
  if (d.kind === "test") {
    return {
      kind: "test",
      questions: d.testQuestions.map((q) => {
        const kept = q.options.filter((o) => filled(o.text));
        return {
          text: q.text.trim(),
          options: kept.map((o) => o.text.trim()),
          correctIndex: Math.max(0, kept.findIndex((o) => o.id === q.correctOptionId)),
        };
      }),
    };
  }
  if (d.kind === "listening") {
    return {
      kind: "listening",
      materialTitle: d.listeningMaterial.trim(),
      questions: d.listeningPrompts.filter((p) => filled(p.text)).map((p) => p.text.trim()),
    };
  }
  return { kind: "reading", text: d.readingText.trim() };
}

const plural = (n: number, one: string, few: string, many: string) => {
  const m10 = n % 10;
  const m100 = n % 100;
  return `${n} ${m10 === 1 && m100 !== 11 ? one : m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14) ? few : many}`;
};

/** One-line description for the library list. */
export function summarize(c: AssignmentContent): string {
  if (c.kind === "test") return plural(c.questions.length, "вопрос", "вопроса", "вопросов");
  if (c.kind === "listening") return c.questions.length > 0 ? `${c.materialTitle} · ${plural(c.questions.length, "вопрос", "вопроса", "вопросов")}` : c.materialTitle;
  return plural(c.text.length, "знак", "знака", "знаков");
}
