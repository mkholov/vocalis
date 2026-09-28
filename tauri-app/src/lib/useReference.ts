import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { readReferenceRecording, type ReferenceRecordingDto } from "./commands";

/**
 * The teacher's reference ("модельное произношение") cached for this student, kept live: fetched once on
 * mount (`readReferenceRecording`) and refreshed by the real `"reference-updated"` event
 * (`commands/student_session.rs`'s own poller, fired the moment a new one finishes being captured) so
 * "Сравнение с эталоном" updates itself without the student leaving and re-entering the recording panel.
 * `null` before any material has been played this session — not an error, just nothing to compare yet.
 */
export function useReference() {
  const [reference, setReference] = useState<ReferenceRecordingDto | null>(null);

  useEffect(() => {
    let cancelled = false;
    readReferenceRecording()
      .then((dto) => {
        if (!cancelled) setReference(dto);
      })
      .catch(() => {});

    let unlisten: (() => void) | undefined;
    listen<ReferenceRecordingDto | null>("reference-updated", (event) => {
      setReference(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return reference;
}
