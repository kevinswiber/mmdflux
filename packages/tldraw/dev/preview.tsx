import React from "react";
import { createRoot } from "react-dom/client";
import { createTLStore, parseTldrawJsonFile, Tldraw } from "tldraw";
import "tldraw/tldraw.css";

function getDataFromUrl():
  | { type: "data"; file: object }
  | { type: "id"; id: string }
  | null {
  const params = new URLSearchParams(window.location.search);
  const id = params.get("id");
  if (id) return { type: "id", id };

  const data = params.get("data");
  if (!data) return null;
  try {
    const json = atob(data);
    return { type: "data", file: JSON.parse(json) as object };
  } catch {
    return null;
  }
}

async function fetchDiagramById(id: string): Promise<object | null> {
  const res = await fetch(`/api/diagram/${encodeURIComponent(id)}`, {
    cache: "no-store",
  });
  if (!res.ok) return null;
  return (await res.json()) as object;
}

function loadStore(file: object) {
  const schema = createTLStore().schema;
  const result = parseTldrawJsonFile({
    json: JSON.stringify(file),
    schema,
  });
  if (!result.ok) {
    throw new Error(`Parse failed: ${JSON.stringify(result.error)}`);
  }
  return result.value;
}

function App() {
  const [error, setError] = React.useState<string | null>(null);
  const [store, setStore] = React.useState<ReturnType<typeof loadStore> | null>(
    null,
  );

  React.useEffect(() => {
    let cancelled = false;

    async function load() {
      const fromUrl = getDataFromUrl();
      let file: object | null = null;

      if (fromUrl?.type === "data") {
        file = fromUrl.file;
      } else if (fromUrl?.type === "id") {
        file = await fetchDiagramById(fromUrl.id);
      }

      if (cancelled) return;
      if (!file) {
        setError(
          fromUrl?.type === "id"
            ? "Diagram not found. It may have been replaced. Run the pipeline again with --open."
            : "No diagram. Use --open after piping MMDS JSON, or open with ?id=<id> or ?data=<base64>.",
        );
        return;
      }

      try {
        setStore(loadStore(file));
        setError(null);
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, []);

  if (error && !store) {
    return (
      <div
        style={{
          padding: "2rem",
          fontFamily: "monospace",
          whiteSpace: "pre-wrap",
          color: "#c00",
        }}
      >
        {error}
      </div>
    );
  }

  if (!store) {
    return (
      <div style={{ padding: "2rem", fontFamily: "sans-serif" }}>Loading…</div>
    );
  }

  return (
    <Tldraw
      store={store}
      onMount={(editor) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            editor.zoomToFit({ immediate: true });
          });
        });
      }}
    />
  );
}

const el = document.getElementById("root");
if (!el) throw new Error("missing #root");
const root = createRoot(el);
root.render(<App />);
