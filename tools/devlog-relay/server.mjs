// FlowRad Mic — dev-loop relay.
//
// Bridges two clients so Claude (in the cloud) can drive + observe the app
// (running on the user's Windows machine) during development:
//
//   Claude  ──MCP over HTTP──▶  RELAY  ◀──WebSocket (outbound)──  app (dev)
//
// The app connects OUT to this relay (so no inbound tunnel to the user's box is
// needed), streams structured events up, and executes commands sent down.
// Claude connects as an MCP client and calls tools to send input + read the
// event stream. Everything is IN MEMORY (a ring buffer + a pending-command map)
// — no database. Auth is a single shared secret (RELAY_TOKEN).
//
// SECURITY: `send_input` drives the user's computer. This relay must only ever
// run with a strong RELAY_TOKEN, and the app-side bridge must be dev-only.

import { randomUUID } from 'node:crypto'
import express from 'express'
import { WebSocketServer } from 'ws'
import { z } from 'zod'
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js'

const PORT = process.env.PORT || 8787
const TOKEN = process.env.RELAY_TOKEN || ''
const RING_CAP = 1000
const COMMAND_TIMEOUT_MS = 30_000

if (!TOKEN) {
  console.error('FATAL: RELAY_TOKEN is required (a strong shared secret).')
  process.exit(1)
}

// ── In-memory state (no DB) ──────────────────────────────────────────────────
/** Ring buffer of the most recent app events. */
const events = []
let seq = 0
function pushEvent(evt) {
  seq += 1
  const stamped = { seq, at: new Date().toISOString(), ...evt }
  events.push(stamped)
  if (events.length > RING_CAP) events.splice(0, events.length - RING_CAP)
  return stamped
}

/** The single connected app socket (last writer wins), or null. */
let appSocket = null
let appInfo = null
/** id -> {resolve} for commands awaiting an app reply. */
const pending = new Map()

function appConnected() {
  return appSocket && appSocket.readyState === 1 /* OPEN */
}

// ── WebSocket endpoint for the app (outbound connection from the user's box) ──
const wss = new WebSocketServer({ noServer: true })
wss.on('connection', (ws) => {
  appSocket = ws
  appInfo = { connectedAt: new Date().toISOString() }
  pushEvent({ kind: 'relay', text: 'app connected' })

  ws.on('message', (raw) => {
    let msg
    try {
      msg = JSON.parse(raw.toString())
    } catch {
      return
    }
    // A reply to a command Claude sent.
    if (msg.type === 'reply' && msg.id && pending.has(msg.id)) {
      pending.get(msg.id).resolve(msg.result ?? {})
      pending.delete(msg.id)
    }
    // A streamed log/trace event.
    if (msg.type === 'event' && msg.event) {
      pushEvent(msg.event)
    }
    if (msg.type === 'hello') {
      appInfo = { ...appInfo, ...msg.info }
      pushEvent({ kind: 'relay', text: 'app hello', info: msg.info })
    }
  })

  ws.on('close', () => {
    if (appSocket === ws) appSocket = null
    pushEvent({ kind: 'relay', text: 'app disconnected' })
  })
})

/** Send a command to the app and await its reply (or time out). */
function sendCommand(command) {
  return new Promise((resolve, reject) => {
    if (!appConnected()) {
      reject(new Error('app is not connected to the relay'))
      return
    }
    const id = randomUUID()
    const timer = setTimeout(() => {
      pending.delete(id)
      reject(new Error(`command timed out after ${COMMAND_TIMEOUT_MS}ms`))
    }, COMMAND_TIMEOUT_MS)
    pending.set(id, {
      resolve: (result) => {
        clearTimeout(timer)
        resolve(result)
      },
    })
    appSocket.send(JSON.stringify({ type: 'command', id, ...command }))
  })
}

// ── MCP server (the tools Claude calls) ──────────────────────────────────────
function buildMcpServer() {
  const server = new McpServer({ name: 'flowrad-devlog', version: '0.1.0' })

  server.tool(
    'app_status',
    'Whether the FlowRad app is connected to the relay, and basic info.',
    {},
    async () => ({
      content: [
        {
          type: 'text',
          text: JSON.stringify(
            { connected: appConnected(), app: appInfo, events_buffered: events.length },
            null,
            2,
          ),
        },
      ],
    }),
  )

  server.tool(
    'send_input',
    'Drive the running app: run TEXT as an Act command (as if spoken). Returns the app’s reply. DEV ONLY — this controls the user’s computer.',
    { text: z.string().describe('The command to run, e.g. "open notepad"') },
    async ({ text }) => {
      try {
        const result = await sendCommand({ action: 'act', text })
        return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] }
      } catch (e) {
        return { isError: true, content: [{ type: 'text', text: String(e.message || e) }] }
      }
    },
  )

  server.tool(
    'get_recent_events',
    'The most recent app events (routing decisions, steps, UIA timings, results, errors). Newest last.',
    {
      limit: z.number().int().min(1).max(RING_CAP).default(80),
      since_seq: z.number().int().optional().describe('Only events after this seq number.'),
    },
    async ({ limit, since_seq }) => {
      let out = events
      if (since_seq != null) out = out.filter((e) => e.seq > since_seq)
      out = out.slice(-limit)
      return { content: [{ type: 'text', text: JSON.stringify(out, null, 2) }] }
    },
  )

  server.tool(
    'get_last_trace',
    'The full event trace of the most recent Act command (all steps grouped by command id).',
    {},
    async () => {
      // Find the last command id seen, then all events sharing it.
      const withCmd = events.filter((e) => e.cmd)
      const lastCmd = withCmd.length ? withCmd[withCmd.length - 1].cmd : null
      const trace = lastCmd ? events.filter((e) => e.cmd === lastCmd) : []
      return {
        content: [{ type: 'text', text: JSON.stringify({ cmd: lastCmd, trace }, null, 2) }],
      }
    },
  )

  server.tool(
    'get_errors',
    'Recent error/failure events only.',
    { limit: z.number().int().min(1).max(200).default(40) },
    async ({ limit }) => {
      const errs = events
        .filter((e) => e.kind === 'error' || e.ok === false || e.result === 'fail')
        .slice(-limit)
      return { content: [{ type: 'text', text: JSON.stringify(errs, null, 2) }] }
    },
  )

  return server
}

// ── HTTP: MCP transport (stateful sessions) + WS upgrade + health ────────────
const app = express()
app.use(express.json({ limit: '2mb' }))

function checkAuth(req, res) {
  const auth = req.headers['authorization'] || ''
  const bearer = auth.startsWith('Bearer ') ? auth.slice(7) : ''
  const headerTok = req.headers['x-relay-token']
  if (bearer === TOKEN || headerTok === TOKEN) return true
  res.status(401).json({ error: 'unauthorized' })
  return false
}

app.get('/health', (_req, res) => res.json({ ok: true, connected: appConnected() }))

const transports = {}
app.post('/mcp', async (req, res) => {
  if (!checkAuth(req, res)) return
  const sid = req.headers['mcp-session-id']
  let transport = sid ? transports[sid] : undefined
  if (!transport) {
    transport = new StreamableHTTPServerTransport({
      sessionIdGenerator: () => randomUUID(),
      onsessioninitialized: (id) => {
        transports[id] = transport
      },
    })
    transport.onclose = () => {
      if (transport.sessionId) delete transports[transport.sessionId]
    }
    await buildMcpServer().connect(transport)
  }
  await transport.handleRequest(req, res, req.body)
})

async function replaySession(req, res) {
  if (!checkAuth(req, res)) return
  const sid = req.headers['mcp-session-id']
  const transport = sid ? transports[sid] : undefined
  if (!transport) {
    res.status(400).send('no session')
    return
  }
  await transport.handleRequest(req, res)
}
app.get('/mcp', replaySession)
app.delete('/mcp', replaySession)

const httpServer = app.listen(PORT, () => {
  console.log(`devlog-relay listening on :${PORT} (MCP /mcp, WS /agent, health /health)`)
})

httpServer.on('upgrade', (req, socket, head) => {
  const url = new URL(req.url, `http://localhost`)
  if (url.pathname !== '/agent') {
    socket.destroy()
    return
  }
  if (url.searchParams.get('token') !== TOKEN) {
    socket.write('HTTP/1.1 401 Unauthorized\r\n\r\n')
    socket.destroy()
    return
  }
  wss.handleUpgrade(req, socket, head, (ws) => wss.emit('connection', ws, req))
})
