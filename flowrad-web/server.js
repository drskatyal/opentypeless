// Minimal dependency-free static server for the FlowRad site.
// Injects the STT API base URL (from env) into the page at request time so the
// live demo knows where to send audio. No build step, Railway-friendly.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, normalize, extname } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PUBLIC = join(__dirname, "public");
const PORT = Number(process.env.PORT ?? 3000);

// Where the browser demo posts audio. Empty => demo shows a "not configured" state.
const API_BASE = process.env.FLOWRAD_API_BASE ?? "";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".webmanifest": "application/manifest+json",
};

const server = createServer(async (req, res) => {
  try {
    const urlPath = decodeURIComponent((req.url ?? "/").split("?")[0]);
    let rel = urlPath === "/" ? "/index.html" : urlPath;
    // Prevent path traversal.
    const filePath = normalize(join(PUBLIC, rel));
    if (!filePath.startsWith(PUBLIC)) {
      res.writeHead(403).end("Forbidden");
      return;
    }
    let data = await readFile(filePath);
    const ext = extname(filePath);
    if (ext === ".html") {
      // Inject the API base as a global the demo reads.
      data = Buffer.from(
        data
          .toString("utf8")
          .replace("%FLOWRAD_API_BASE%", JSON.stringify(API_BASE)),
      );
    }
    res.writeHead(200, { "content-type": TYPES[ext] ?? "application/octet-stream" });
    res.end(data);
  } catch {
    // SPA-ish fallback: unknown paths return the home page.
    try {
      const home = (await readFile(join(PUBLIC, "index.html")))
        .toString("utf8")
        .replace("%FLOWRAD_API_BASE%", JSON.stringify(API_BASE));
      res.writeHead(200, { "content-type": TYPES[".html"] }).end(home);
    } catch {
      res.writeHead(404).end("Not found");
    }
  }
});

server.listen(PORT, "0.0.0.0", () => {
  console.log(`flowrad-web listening on :${PORT}`);
});
