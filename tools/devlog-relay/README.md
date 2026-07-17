# FlowRad Mic — dev-loop relay

A tiny relay that lets Claude (in the cloud) **drive and observe** the app while
it runs on your Windows machine during development — so bugs can be reproduced,
traced, and fixed without copy-pasting logs.

```
Claude ──MCP over HTTP──▶  RELAY (Railway)  ◀──WebSocket (outbound)──  app (dev)
        send_input / get_last_trace / get_errors        streams events, runs commands
```

In-memory only (a ring buffer + a pending-command map) — **no database**. The app
connects **outbound** to the relay, so nothing needs to be exposed on your box.

## Security (read this)

`send_input` executes commands that **drive your computer**. Therefore:

- `RELAY_TOKEN` is a **required** strong shared secret. The app and Claude both
  present it; without it, connections are rejected.
- Run the app-side bridge in **dev builds only**.
- Deploy to **your own** Railway project.
- The app-side bridge must never forward password-field values / secret-shaped
  inputs (same discipline as the rest of Act).

## Env

| Var | Required | Default | Notes |
|---|---|---|---|
| `RELAY_TOKEN` | yes | — | strong shared secret |
| `PORT` | no | 8787 | Railway sets this automatically |

## Run locally

```bash
npm install
RELAY_TOKEN=dev-secret npm start
# smoke test (in another shell):
RELAY_TOKEN=dev-secret node smoke.mjs
```

## Deploy to Railway

Deploy this directory (`tools/devlog-relay`) as the service root. Set
`RELAY_TOKEN` in the service variables. Railway provides `PORT`. Health check:
`GET /health`.

## Add to Claude as an MCP connector

In claude.ai → Settings → Connectors → Add custom connector:

- URL: `https://<your-railway-app>.up.railway.app/mcp`
- Auth: Bearer token = your `RELAY_TOKEN` (or header `X-Relay-Token`).

Then start a session; the tools below appear.

## MCP tools (what Claude calls)

- `app_status()` — is the app connected, basic info, buffered event count.
- `send_input({ text })` — run `text` as an Act command on the app; returns the
  app's reply.
- `get_recent_events({ limit, since_seq })` — recent events, newest last.
- `get_last_trace()` — all events for the most recent command id.
- `get_errors({ limit })` — error/failure events only.

## App ↔ relay wire protocol (WebSocket `/agent?token=…`)

The Rust dev-bridge in the app implements this.

App → relay:
```jsonc
{ "type": "hello",  "info": { "platform": "windows", "version": "0.1.42" } }
{ "type": "event",  "event": { "cmd": "c3", "stage": "route", "transcript": "…", "missions": ["…"] } }
{ "type": "event",  "event": { "cmd": "c3", "step": "t0", "action": "launch", "target": "Chrome", "ok": true, "ms": 420 } }
{ "type": "reply",  "id": "<command-id>", "result": { "ok": true, "summary": "…" } }
```

Relay → app:
```jsonc
{ "type": "command", "id": "<uuid>", "action": "act", "text": "open notepad" }
```

Events are free-form JSON objects; include a stable `cmd` id per command so
`get_last_trace` can group them. Include `ok:false` / `kind:"error"` on failures
so `get_errors` finds them.
