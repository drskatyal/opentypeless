/**
 * The upstream transcription engine. Isolated here so the rest of the codebase —
 * and every public response — refers only to FlowRad tiers, never the provider.
 * The provider name and endpoint live ONLY in this file and the environment.
 */
import { buildSystemInstruction, type PromptOptions } from "./prompt.js";

/** Public tiers exposed to clients. */
export type Tier = "fast" | "precise";

/** Internal tier → upstream model id. Never sent to clients. */
const MODEL_BY_TIER: Record<Tier, string> = {
  // Primary: fastest, the default for dictation.
  fast: "gemini-3.1-flash-lite",
  // Secondary: higher accuracy fallback.
  precise: "gemini-3.5-flash",
};

const UPSTREAM_BASE =
  process.env.FLOWRAD_UPSTREAM_BASE ??
  "https://generativelanguage.googleapis.com/v1beta";

export function isTier(v: unknown): v is Tier {
  return v === "fast" || v === "precise";
}

export interface TranscribeInput {
  audioBase64: string;
  mimeType: string;
  tier: Tier;
  prompt: PromptOptions;
  /** Hard wall-clock timeout in ms. */
  timeoutMs?: number;
}

export class EngineError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
    this.name = "EngineError";
  }
}

/**
 * Transcribe a single audio clip. Returns plain transcript text ("" if silent).
 * Throws EngineError with a *sanitized* message — upstream error bodies are never
 * forwarded to clients (they can leak the provider identity).
 */
export async function transcribe(input: TranscribeInput): Promise<string> {
  const apiKey = process.env.FLOWRAD_ENGINE_KEY;
  if (!apiKey) throw new EngineError(503, "Transcription engine is not configured.");

  const model = MODEL_BY_TIER[input.tier];
  const url = `${UPSTREAM_BASE}/models/${model}:generateContent`;

  const body = {
    systemInstruction: {
      parts: [{ text: buildSystemInstruction(input.prompt) }],
    },
    contents: [
      {
        role: "user",
        parts: [
          { text: "Transcribe this audio." },
          { inlineData: { mimeType: input.mimeType, data: input.audioBase64 } },
        ],
      },
    ],
    generationConfig: { temperature: 0.0 },
  };

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), input.timeoutMs ?? 60_000);
  let resp: Response;
  try {
    resp = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", "x-goog-api-key": apiKey },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
  } catch (e) {
    throw new EngineError(504, "Transcription engine timed out.");
  } finally {
    clearTimeout(timer);
  }

  if (!resp.ok) {
    // Do NOT forward the upstream body — map to a generic, provider-agnostic error.
    if (resp.status === 429) throw new EngineError(429, "Transcription is busy. Please retry shortly.");
    if (resp.status === 400) throw new EngineError(400, "The audio could not be processed.");
    throw new EngineError(502, "Transcription engine error.");
  }

  const json = (await resp.json()) as unknown;
  return extractText(json);
}

function extractText(json: unknown): string {
  try {
    const parts =
      (json as any)?.candidates?.[0]?.content?.parts;
    if (!Array.isArray(parts)) return "";
    return parts
      .map((p: any) => (typeof p?.text === "string" ? p.text : ""))
      .join("")
      .trim();
  } catch {
    return "";
  }
}
