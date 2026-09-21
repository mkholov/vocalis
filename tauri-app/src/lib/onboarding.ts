const STORAGE_KEY = "vocalis-onboarding-seen";
// If `localStorage` is unavailable (blocked/private mode), the choice can't survive a restart — but it
// still shouldn't pop up again every time the console is re-entered within the same run.
let seenThisRun = false;

/** Has the first-run walkthrough already been shown (finished or skipped)? */
export function hasSeenOnboarding(): boolean {
  if (seenThisRun) return true;
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function markOnboardingSeen() {
  seenThisRun = true;
  try {
    localStorage.setItem(STORAGE_KEY, "1");
  } catch {
    // remembered for this run only
  }
}
