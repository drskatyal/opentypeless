import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

/** Live status of one Act mission ("task") on the Agents board. `queued` is a
 * mission that has been spawned (all of a command's agents appear at once, up
 * front) but hasn't started running yet. */
export type ActTaskStatus = 'queued' | 'running' | 'done' | 'failed'

/** One card on the Agents board — a single mission of the current command. */
export interface ActTask {
  id: string
  label: string
  status: ActTaskStatus
  /** Latest streamed progress line while running. */
  detail?: string
  /** Final one-line result once done/failed. */
  summary?: string
}

type Unlisten = () => void | Promise<void>

// Only the task lifecycle + the command-start signal are relevant here; every
// other ActEvent kind is ignored. Mirrors the wire format from act/events.rs.
type TaskEvent =
  | { kind: 'state'; state: string }
  | { kind: 'task_spawned'; id: string; label: string; status?: string }
  | { kind: 'task_progress'; id: string; text: string }
  | { kind: 'task_result'; id: string; ok: boolean; summary: string }
  | { kind: string; [k: string]: unknown }

function safeUnlisten(unlisten: Unlisten) {
  try {
    Promise.resolve(unlisten()).catch(() => {})
  } catch {
    // Dev HMR can leave Tauri listener handles stale.
  }
}

/**
 * Subscribe to the Act event stream and reduce it to the current command's task
 * list. A `state: working` event marks the start of a fresh command and clears
 * the previous run's cards; task_spawned/progress/result then build them up.
 *
 * State is local to this hook (not the global store) so the board is a pure view
 * of the live event stream.
 */
export function useActTasks(): ActTask[] {
  const [tasks, setTasks] = useState<ActTask[]>([])

  useEffect(() => {
    let cancelled = false
    let unlisten: Unlisten | null = null

    listen<TaskEvent>('act://event', (event) => {
      const payload = event.payload
      switch (payload.kind) {
        case 'state':
          // A new command is starting — drop the previous run's cards.
          if ((payload as { state: string }).state === 'working') {
            setTasks([])
          }
          break
        case 'task_spawned': {
          const p = payload as { id: string; label: string; status?: string }
          // All of a command's agents are spawned up front as `queued`; each
          // lights up (flips to `running`) when its mission starts. A spawn with
          // no status defaults to `running` — back-compat with the old lazy spawn.
          const status: ActTaskStatus = p.status === 'queued' ? 'queued' : 'running'
          setTasks((prev) =>
            prev.some((t) => t.id === p.id)
              ? prev
              : [...prev, { id: p.id, label: p.label, status }],
          )
          break
        }
        case 'task_progress': {
          const p = payload as { id: string; text: string }
          // The first progress line is a mission's "started" signal — flip a still
          // -queued card to running as its detail arrives.
          setTasks((prev) =>
            prev.map((t) =>
              t.id === p.id
                ? { ...t, detail: p.text, status: t.status === 'queued' ? 'running' : t.status }
                : t,
            ),
          )
          break
        }
        case 'task_result': {
          const p = payload as { id: string; ok: boolean; summary: string }
          setTasks((prev) =>
            prev.map((t) =>
              t.id === p.id ? { ...t, status: p.ok ? 'done' : 'failed', summary: p.summary } : t,
            ),
          )
          break
        }
        default:
          break
      }
    })
      .then((fn) => {
        if (cancelled) safeUnlisten(fn)
        else unlisten = fn
      })
      .catch((err) => console.error('Failed to register Act tasks listener:', err))

    return () => {
      cancelled = true
      if (unlisten) safeUnlisten(unlisten)
    }
  }, [])

  return tasks
}
