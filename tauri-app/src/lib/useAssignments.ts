import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AssignmentOfferDto } from "./commands";

/** Real assignments this student has received this session — listens for the `"assignments"` event
 * `commands/student_session.rs`'s poller emits (the whole current list, each time a real
 * `ServerToClient::AssignmentOffer` arrives), the same event a real connected teacher's `sendAssignment`
 * triggers. Starts empty; nothing to fetch on mount, since there's nothing until the teacher sends
 * something — this only ever grows for the life of the session (a fresh connect starts over). */
export function useAssignments() {
  const [assignments, setAssignments] = useState<AssignmentOfferDto[]>([]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<AssignmentOfferDto[]>("assignments", (event) => setAssignments(event.payload)).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return assignments;
}
