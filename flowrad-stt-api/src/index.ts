import Fastify from "fastify";
import cors from "@fastify/cors";
import rateLimit from "@fastify/rate-limit";
import { transcribe, isTier, EngineError, type Tier } from "./engine.js";

const PORT = Number(process.env.PORT ?? 8080);
const HOST = "0.0.0.0";

// Comma-separated allowed web origins for the browser demo/site.
const ALLOWED_ORIGINS = (process.env.FLOWRAD_ALLOWED_ORIGINS ?? "")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

// Optional shared token the desktop app sends; if unset, the /v1/transcribe
// route is open (still rate-limited). The demo route is always open.
const APP_TOKEN = process.env.FLOWRAD_APP_TOKEN ?? "";

// Demo guardrails (medium): per-IP rate limit + hard clip-length cap.
const DEMO_MAX_SECONDS = Number(process.env.FLOWRAD_DEMO_MAX_SECONDS ?? 20);
const MAX_BODY_BYTES = Number(process.env.FLOWRAD_MAX_BODY_BYTES ?? 8 * 1024 * 1024);

const app = Fastify({
  logger: true,
  bodyLimit: MAX_BODY_BYTES,
  trustProxy: true, // Railway sits behind a proxy; needed for correct client IPs.
});

await app.register(cors, {
  origin: ALLOWED_ORIGINS.length ? ALLOWED_ORIGINS : true,
  methods: ["POST", "GET", "OPTIONS"],
});

await app.register(rateLimit, {
  global: false,
  keyGenerator: (req) => req.ip,
});

interface TranscribeBody {
  audio?: string; // base64 (no data: prefix)
  mimeType?: string;
  tier?: string;
  language?: string;
  appContext?: string;
}

/** Approx clip seconds from a base64 WAV/PCM16 mono 16kHz payload. */
function approxSeconds(base64: string): number {
  const decodedBytes = Math.floor((base64.length * 3) / 4);
  // 16kHz * 16-bit * 1ch = 32000 bytes/sec; minus a 44-byte header is noise.
  return decodedBytes / 32000;
}

function parseBody(body: TranscribeBody): {
  audioBase64: string;
  mimeType: string;
  tier: Tier;
  language?: string;
  appContext?: string;
} {
  const audioBase64 = typeof body.audio === "string" ? body.audio.trim() : "";
  if (!audioBase64) throw new EngineError(400, "Missing audio.");
  const mimeType =
    typeof body.mimeType === "string" && body.mimeType ? body.mimeType : "audio/wav";
  const tier: Tier = isTier(body.tier) ? body.tier : "fast";
  return {
    audioBase64,
    mimeType,
    tier,
    language: typeof body.language === "string" ? body.language : undefined,
    appContext: typeof body.appContext === "string" ? body.appContext : undefined,
  };
}

app.get("/health", async () => ({ ok: true, service: "flowrad-stt-api" }));

// ---- App endpoint: for the FlowRad desktop app (optional bearer token) ----
app.post<{ Body: TranscribeBody }>(
  "/v1/transcribe",
  { config: { rateLimit: { max: 120, timeWindow: "1 minute" } } },
  async (req, reply) => {
    if (APP_TOKEN) {
      const auth = req.headers.authorization ?? "";
      if (auth !== `Bearer ${APP_TOKEN}`) {
        return reply.code(401).send({ error: "Unauthorized." });
      }
    }
    const p = parseBody(req.body ?? {});
    try {
      const text = await transcribe({
        audioBase64: p.audioBase64,
        mimeType: p.mimeType,
        tier: p.tier,
        prompt: { language: p.language, appContext: p.appContext },
      });
      return { text };
    } catch (e) {
      return sendEngineError(reply, e);
    }
  },
);

// ---- Public demo endpoint: no auth, tighter limits + clip cap ----
app.post<{ Body: TranscribeBody }>(
  "/v1/demo/transcribe",
  { config: { rateLimit: { max: 12, timeWindow: "1 minute" } } },
  async (req, reply) => {
    const p = parseBody(req.body ?? {});
    const secs = approxSeconds(p.audioBase64);
    if (secs > DEMO_MAX_SECONDS) {
      return reply
        .code(413)
        .send({ error: `Demo clips are limited to ${DEMO_MAX_SECONDS}s. Download FlowRad Mic for unlimited use.` });
    }
    try {
      const text = await transcribe({
        audioBase64: p.audioBase64,
        mimeType: p.mimeType,
        tier: p.tier,
        prompt: { language: p.language }, // demo ignores appContext
      });
      return { text };
    } catch (e) {
      return sendEngineError(reply, e);
    }
  },
);

function sendEngineError(reply: any, e: unknown) {
  if (e instanceof EngineError) return reply.code(e.status).send({ error: e.message });
  reply.log.error(e);
  return reply.code(500).send({ error: "Unexpected error." });
}

app
  .listen({ port: PORT, host: HOST })
  .then((addr) => app.log.info(`flowrad-stt-api listening on ${addr}`))
  .catch((err) => {
    app.log.error(err);
    process.exit(1);
  });
