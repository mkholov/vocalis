import { useCallback, useEffect, useRef, useState } from "react";
import { deleteRecording, listRecordings, readRecording, startRecording, stopRecording, type RecordingDto } from "./commands";

/** The student's own voice recordings (step 7.5 item 6 — `vocalis_roadmap.md`,
 * section 8): record, list, play back in-app, delete. All of it real —
 * `commands/student_recording.rs` reuses `student::recording` and the recording
 * tap in `student::audio` unchanged; nothing here is a mock. Needs the live
 * student session (`useStudentSession`) only for *recording* (that's where the
 * mic capture is); the list/playback/delete only touch files on disk.
 *
 * `error` carries what the backend said — notably "микрофон недоступен" on a
 * machine with no input device — rather than throwing. Leaving the screen
 * mid-recording doesn't lose it: the backend saves an in-progress recording on
 * disconnect, and this also stops it on unmount. */
export function useRecordings() {
  const [recordings, setRecordings] = useState<RecordingDto[]>([]);
  const [recording, setRecording] = useState(false);
  const [elapsedSecs, setElapsedSecs] = useState(0);
  const [playing, setPlaying] = useState<string | null>(null);
  const [error, setError] = useState<string | undefined>();
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const recordingRef = useRef(false);

  const refresh = useCallback(() => {
    return listRecordings()
      .then(setRecordings)
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);

  // Elapsed-time readout while recording — purely a UI clock; the real length
  // is whatever the backend actually captured.
  useEffect(() => {
    if (!recording) return;
    const startedAt = Date.now();
    setElapsedSecs(0);
    const id = setInterval(() => setElapsedSecs(Math.floor((Date.now() - startedAt) / 1000)), 250);
    return () => clearInterval(id);
  }, [recording]);

  useEffect(() => {
    return () => {
      audioRef.current?.pause();
      if (recordingRef.current) stopRecording().catch(() => {});
    };
  }, []);

  async function toggleRecording() {
    if (recording) {
      try {
        await stopRecording();
        setError(undefined);
      } catch (err) {
        setError(String(err));
      }
      setRecording(false);
      await refresh();
      return;
    }
    try {
      await startRecording();
      setRecording(true);
      setError(undefined);
    } catch (err) {
      setError(String(err));
    }
  }

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
      const audio = new Audio(await readRecording(name));
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

  async function remove(name: string) {
    if (playing === name) stopPlayback();
    try {
      await deleteRecording(name);
      setError(undefined);
    } catch (err) {
      setError(String(err));
    }
    await refresh();
  }

  return { recordings, recording, elapsedSecs, playing, error, toggleRecording, togglePlay, remove };
}
