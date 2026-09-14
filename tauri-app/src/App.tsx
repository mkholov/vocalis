// Deliberately empty screen — step 2 of the Tauri migration only wires the
// Rust core in as commands (src-tauri/src/commands/), it doesn't call them
// from any screen yet. Try one from the webview's dev console, e.g.
// `await window.__TAURI__.core.invoke("list_classes")`.
function App() {
  return (
    <main className="flex h-screen w-screen items-center justify-center bg-neutral-950">
      <h1 className="text-4xl font-semibold text-white">Vocalis</h1>
    </main>
  );
}

export default App;
