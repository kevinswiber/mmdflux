import { createHash } from "node:crypto";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const diagramMap = new Map<string, string>();

function contentId(body: string): string {
  return createHash("sha256").update(body).digest("hex").slice(0, 12);
}

export default defineConfig({
  root: "dev",
  plugins: [
    react(),
    {
      name: "diagram-api",
      enforce: "pre",
      configureServer(server) {
        server.middlewares.use((req, res, next) => {
          if (req.url === "/api/diagram" && req.method === "POST") {
            let body = "";
            req.on("data", (chunk) => (body += chunk));
            req.on("end", () => {
              const id = contentId(body);
              diagramMap.set(id, body);
              res.writeHead(200, { "Content-Type": "application/json" });
              res.end(JSON.stringify({ ok: true, id }));
            });
            return;
          }
          const diagramMatch = req.url?.match(/^\/api\/diagram\/([^/?#]+)/);
          if (diagramMatch) {
            const id = decodeURIComponent(diagramMatch[1]);
            const body = diagramMap.get(id);
            if (body) {
              res.writeHead(200, {
                "Content-Type": "application/json",
                "Cache-Control": "no-store",
              });
              res.end(body);
            } else {
              res.writeHead(404).end();
            }
            return;
          }
          next();
        });
      },
    },
  ],
});
