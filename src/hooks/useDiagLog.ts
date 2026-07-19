import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/** One captured log line — mirrors `LogEntry` in src-tauri/src/diag.rs. */
export interface DiagLogEntry {
  seq: number
  ts_ms: number
  level: 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'TRACE' | string
  target: string
  message: string
}

/** Keep the in-memory view bounded so a long session can't grow it without limit. */
const MAX_ENTRIES = 4000

type Unlisten = () => void

/**
 * Subscribe to the in-app diagnostics stream: seed with the backend ring buffer
 * (`diag_log_dump`), then append each live `diag://log` event. Returns the current
 * entries plus a `clear()` that empties both the backend buffer and the view.
 *
 * Only active while `enabled` (the Diagnostics pane is open) to avoid holding a
 * Tauri listener for the whole app lifetime.
 */
export function useDiagLog(enabled: boolean): {
  entries: DiagLogEntry[]
  clear: () => Promise<void>
} {
  const [entries, setEntries] = useState<DiagLogEntry[]>([])
  // Guard against a live event landing between the seed dump and listener setup.
  const seenSeq = useRef<Set<number>>(new Set())

  useEffect(() => {
    if (!enabled) return
    let cancelled = false
    let unlisten: Unlisten | null = null

    const append = (entry: DiagLogEntry) => {
      if (seenSeq.current.has(entry.seq)) return
      seenSeq.current.add(entry.seq)
      setEntries((prev) => {
        const next = prev.length >= MAX_ENTRIES ? prev.slice(prev.length - MAX_ENTRIES + 1) : prev
        return [...next, entry]
      })
    }

    // Register the listener BEFORE the seed dump so nothing is missed; the seq
    // dedupe reconciles any overlap.
    listen<DiagLogEntry>('diag://log', (event) => {
      if (!cancelled) append(event.payload)
    })
      .then((fn) => {
        if (cancelled) fn()
        else unlisten = fn
      })
      .catch((err) => console.error('Failed to register diag log listener:', err))

    invoke<DiagLogEntry[]>('diag_log_dump', { limit: MAX_ENTRIES })
      .then((seed) => {
        if (cancelled) return
        for (const e of seed) append(e)
      })
      .catch((err) => console.error('Failed to load diag log:', err))

    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [enabled])

  const clear = async () => {
    try {
      await invoke('diag_log_clear')
    } catch (err) {
      console.error('Failed to clear diag log:', err)
    }
    seenSeq.current.clear()
    setEntries([])
  }

  return { entries, clear }
}
