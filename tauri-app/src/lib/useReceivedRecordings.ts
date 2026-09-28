import { useCallback, useEffect, useRef, useState } from "react";
import { listReceivedRecordings, readReceivedRecording, type ReceivedRecordingDto } from "./commands";

const POLL_MS = 2000;

/** Recordings students have really sent this run (`commands/teacher_session.rs`'s `list_received_recordings`
 * / `read_received_recording`, reading the same real files `teacher::net::handle_student`'s unchanged
 * `FileOffer` handler writes). There's no push event for "a new one arrived" (unlike chat/assignments), so
 * this polls a plain, cheap directory listing while `active` is true — pass that as "is the drawer/panel
 * actually open" so it doesn't poll in the background for no one. */
export function useReceivedRecordings(active: boolean) {
  const [recordings, setRecordings] = useState<ReceivedRecordingDto[]>([]);
  const [playing, setPlaying] = useState<string | null>(null);
  const [error, setError] = useState<string | undefined>();
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const refresh = useCallback(() => {
    return listReceivedRecordings()
      .then(setRecordings)
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    if (!active) return;
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  }, [active, refresh]);

  useEffect(() => {
    return () => {
      audioRef.current?.pause();
    };
  }, []);

  function stopPlayback() {
    audioRef.current?.pause();
    audioRef.current = null;
    setPlaying(null);
  }

  async function togglePlay(name: string) {
    if (playing === name) {
      stopPlayback();
      return;
    }
    stopPlayback();
    try {
      const audio = new Audio(await readReceivedRecording(name));
      audio.onended = () => setPlaying((cur) => (cur === name ? null : cur));
      audioRef.current = audio;
      setPlaying(name);
      await audio.play();
      setError(undefined);
    } catch (err) {
      setPlaying(null);
      setError(String(err));
    }
  }

  return { recordings, playing, error, togglePlay, refresh };
}
