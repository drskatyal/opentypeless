// Smoke test: start nothing here — assumes server is running on PORT with TOKEN.
// Simulates the APP (WebSocket) and CLAUDE (MCP client) and checks the bridge.
import WebSocket from 'ws'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js'

const PORT = process.env.PORT || 8787
const TOKEN = process.env.RELAY_TOKEN
const base = `http://127.0.0.1:${PORT}`

function assert(cond, msg) {
  if (!cond) {
    console.error('FAIL:', msg)
    process.exit(1)
  }
  console.log('ok:', msg)
}

// 1. Health
const health = await (await fetch(`${base}/health`)).json()
assert(health.ok === true, 'health ok')

// 2. Simulate the app over WS
const app = new WebSocket(`ws://127.0.0.1:${PORT}/agent?token=${TOKEN}`)
await new Promise((res, rej) => {
  app.on('open', res)
  app.on('error', rej)
})
app.send(JSON.stringify({ type: 'hello', info: { platform: 'test', version: '0.1' } }))
app.send(
  JSON.stringify({
    type: 'event',
    event: { cmd: 'c1', stage: 'route', transcript: 'open notepad', missions: ['open_flow:open_app'] },
  }),
)
// App answers commands: echo back a fake result.
app.on('message', (raw) => {
  const msg = JSON.parse(raw.toString())
  if (msg.type === 'command') {
    app.send(
      JSON.stringify({ type: 'event', event: { cmd: 'c2', stage: 'route', transcript: msg.text } }),
    )
    app.send(JSON.stringify({ type: 'reply', id: msg.id, result: { ran: msg.text, ok: true } }))
  }
})
await new Promise((r) => setTimeout(r, 200))

// 3. Connect as Claude via MCP
const transport = new StreamableHTTPClientTransport(new URL(`${base}/mcp`), {
  requestInit: { headers: { Authorization: `Bearer ${TOKEN}` } },
})
const client = new Client({ name: 'smoke', version: '0.1' })
await client.connect(transport)

const tools = await client.listTools()
const names = tools.tools.map((t) => t.name).sort()
assert(
  ['app_status', 'get_errors', 'get_last_trace', 'get_recent_events', 'send_input'].every((n) =>
    names.includes(n),
  ),
  `tools present: ${names.join(', ')}`,
)

const status = JSON.parse((await client.callTool({ name: 'app_status', arguments: {} })).content[0].text)
assert(status.connected === true, 'app_status shows connected')

const recent = JSON.parse(
  (await client.callTool({ name: 'get_recent_events', arguments: { limit: 20 } })).content[0].text,
)
assert(
  recent.some((e) => e.transcript === 'open notepad'),
  'get_recent_events sees the streamed app event',
)

const sent = JSON.parse(
  (await client.callTool({ name: 'send_input', arguments: { text: 'launch spotify' } })).content[0]
    .text,
)
assert(sent.ran === 'launch spotify' && sent.ok === true, 'send_input drove the app and got a reply')

const trace = JSON.parse((await client.callTool({ name: 'get_last_trace', arguments: {} })).content[0].text)
assert(trace.cmd === 'c2' && trace.trace.length >= 1, 'get_last_trace groups by command id')

console.log('\nALL SMOKE CHECKS PASSED')
await client.close()
app.close()
process.exit(0)
