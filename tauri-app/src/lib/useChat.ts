import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export interface ReceivedChatMessage {
  id: number;
  from: string;
  text: string;
}

/** Real incoming chat — the `"chat-message"` event (`commands/teacher_session.rs`'s and `commands/
 * student_session.rs`'s own pollers, the exact same `{from, text}` shape on both sides, straight off the
 * real, unchanged `SharedState`/`AppState.chat_log` `teacher::net`/`student::net` already write to), so one
 * hook serves both `ChatDrawer` (teacher) and the student console. Starts empty on every mount — a fresh
 * session's own history — and only ever grows with the *other* side's messages: each side's own send
 * command never touches its own `chat_log`, so a caller's own sent messages are never echoed back here and
 * must be appended locally by the caller itself. */
export function useChat() {
  const [messages, setMessages] = useState<ReceivedChatMessage[]>([]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    let nextId = 1;
    listen<{ from: string; text: string }>("chat-message", (event) => {
      setMessages((prev) => [...prev, { id: nextId++, from: event.payload.from, text: event.payload.text }]);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return messages;
}
