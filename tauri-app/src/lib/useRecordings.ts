import { useCallback, useEffect, useRef, useState } from "react";
import { deleteRecording, listRecordings, readRecording, sendRecordingToTeacher, startRecording, stopRecording, type RecordingDto } from "./commands";

/** The sentinel `playing` takes to mean "the reference recording, not one of `recordings`" — lets a single
 * play/pause slot (and a single `<audio>` element) cover both, so starting one always stops the other. No
 * real recording name can ever collide with it (real names are `recording_<epoch>.wav`). */
const REFERENCE_SLOT = "__reference__";

/** The student's own voice recordings (step 7.5 item 6 — `vocalis_roadmap.md`,
 * section 8): record, list, play back in-app, delete, compare against the teacher's reference, and send to
 * the teacher. All of it real — `commands/student_recording.rs` reuses `student::recording` and the
 * recording tap in `student::audio` unchanged, the reference comes from a real (unchanged) `student::net`
 * capture, and sending is a real `ClientToServer::FileOffer`; nothing here is a mock. Needs the live
 * student session (`useStudentSession`) for *recording* (that's where the mic capture is) and for *sending*
 * (that's where the network connection is); list/playback/delete only touch files on disk.
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
  const [sendingTo, setSendingTo] = useState<string | null>(null);
  const [sentNames, setSentNames] = useState<Set<string>>(new Set());
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

  /** Plays the teacher's reference recording — same play/pause slot as `togglePlay`, so it's mutually
   * exclusive with playing one of the student's own recordings, which is the point: "прослушать оба
   * подряд/переключаясь в одном месте" means only one plays at a time. */
  async function togglePlayReference(dataUrl: string) {
    if (playing === REFERENCE_SLOT) {
      stopPlayback();
      return;
    }
    stopPlayback();
    try {
      const audio = new Audio(dataUrl);
      audio.onended = () => setPlaying((cur) => (cur === REFERENCE_SLOT ? null : cur));
      audioRef.current = audio;
      setPlaying(REFERENCE_SLOT);
      await audio.play();
      setError(undefined);
    } catch (err) {
      setPlaying(null);
      setError(String(err));
    }
  }

  /** Sends a saved recording to the teacher for real (`ClientToServer::FileOffer`). `sentNames` just tracks
   * "sent at least once this screen visit" for a checkmark — re-sending is allowed (e.g. after re-recording
   * a take with the same idea in mind), it isn't a one-shot lock like assignment submission. */
  async function sendToTeacher(name: string) {
    setSendingTo(name);
    try {
      await sendRecordingToTeacher(name);
      setSentNames((prev) => new Set(prev).add(name));
      setError(undefined);
    } catch (err) {
      setError(String(err));
    } finally {
      setSendingTo(null);
    }
  }

  return {
    recordings,
    recording,
    elapsedSecs,
    playing,
    isPlayingReference: playing === REFERENCE_SLOT,
    error,
    sendingTo,
    sentNames,
    toggleRecording,
    togglePlay,
    togglePlayReference,
    remove,
    sendToTeacher,
  };
}
