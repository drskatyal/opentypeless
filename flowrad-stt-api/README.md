# flowrad-stt-api

Proprietary FlowRad speech-to-text API. A thin, hardened transcription proxy:
clients send audio, receive text. The upstream provider and key are never
exposed — clients only ever see FlowRad tiers (`fast`, `precise`).

## Endpoints

- `GET /health` — liveness.
- `POST /v1/transcribe` — app endpoint (optional `Authorization: Bearer <token>`).
- `POST /v1/demo/transcribe` — public website demo. Rate-limited, clip length capped.

### Request body (JSON)

```json
{
  "audio": "<base64 WAV, no data: prefix>",
  "mimeType": "audio/wav",
  "tier": "fast",              // "fast" (default) | "precise"
  "language": "English (United States)",   // optional
  "appContext": "radiology report field"   // optional; ignored by the demo
}
```

Response: `{ "text": "..." }`.

## Security

- Audio is treated strictly as data. The system instruction is injection-hardened
  (see `src/prompt.ts`): spoken "commands" are transcribed, never obeyed.
- Upstream error bodies are never forwarded — responses are provider-agnostic.
- The engine has no tools and cannot execute actions.

## Deploy (Railway)

Set `FLOWRAD_ENGINE_KEY` in Railway Variables. Nixpacks builds via `railway.json`.

## Local

```bash
npm install
cp .env.example .env   # fill FLOWRAD_ENGINE_KEY
npm run dev
```
