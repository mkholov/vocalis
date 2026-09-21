/** Puts `text` on the system clipboard. `navigator.clipboard` first (works in both WebView2 and WKWebView
 * when called from a click); the old hidden-textarea `execCommand("copy")` as a fallback for a webview
 * that refuses the async API. Resolves `false` — never throws — if neither worked, so the caller can say so. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // fall through to the legacy path
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  try {
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    document.body.removeChild(area);
  }
}
