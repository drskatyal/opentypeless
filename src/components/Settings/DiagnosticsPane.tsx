import { useMemo, useRef, useState, useEffect } from 'react'
import { Copy, Trash2, ArrowDownToLine, Check } from 'lucide-react'
import { useDiagLog, type DiagLogEntry } from '../../hooks/useDiagLog'

/** Level → row accent. Kept English/enum-keyed (not localized) — these are raw log levels. */
const LEVEL_STYLES: Record<string, string> = {
  ERROR: 'text-red-500',
  WARN: 'text-amber-500',
  INFO: 'text-sky-500',
  DEBUG: 'text-text-tertiary',
  TRACE: 'text-text-tertiary',
}

const LEVELS = ['ERROR', 'WARN', 'INFO', 'DEBUG'] as const

function formatTime(ms: number): string {
  const d = new Date(ms)
  const hh = String(d.getHours()).padStart(2, '0')
  const mm = String(d.getMinutes()).padStart(2, '0')
  const ss = String(d.getSeconds()).padStart(2, '0')
  const mmm = String(d.getMilliseconds()).padStart(3, '0')
  return `${hh}:${mm}:${ss}.${mmm}`
}

/** Short module tail, e.g. `opentypeless_lib::act::conductor` → `act::conductor`. */
function shortTarget(target: string): string {
  return target.replace(/^opentypeless_lib::/, '')
}

function entryToText(e: DiagLogEntry): string {
  return `${formatTime(e.ts_ms)} ${e.level.padEnd(5)} ${shortTarget(e.target)} ${e.message}`
}

/**
 * Diagnostics pane: a live, in-app console mirroring every backend log line —
 * actions, errors, and full LLM prompts/responses — so problems are visible
 * without a terminal attached. Filter by level and text, copy, or clear.
 */
export function DiagnosticsPane() {
  const { entries, clear } = useDiagLog(true)
  const [query, setQuery] = useState('')
  const [levelFilter, setLevelFilter] = useState<Set<string>>(new Set(LEVELS))
  const [autoScroll, setAutoScroll] = useState(true)
  const [copied, setCopied] = useState(false)
  const scrollRef = useRef<HTMLDivElement | null>(null)

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return entries.filter((e) => {
      if (!levelFilter.has(e.level)) return false
      if (!q) return true
      return e.message.toLowerCase().includes(q) || e.target.toLowerCase().includes(q)
    })
  }, [entries, query, levelFilter])

  // Stick to the bottom as new lines arrive, unless the user turned it off.
  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [filtered.length, autoScroll])

  const toggleLevel = (level: string) => {
    setLevelFilter((prev) => {
      const next = new Set(prev)
      if (next.has(level)) next.delete(level)
      else next.add(level)
      return next
    })
  }

  const copyAll = async () => {
    const text = filtered.map(entryToText).join('\n')
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    } catch (err) {
      console.error('Copy failed:', err)
    }
  }

  const errorCount = useMemo(
    () => entries.filter((e) => e.level === 'ERROR' || e.level === 'WARN').length,
    [entries],
  )

  return (
    <div className="flex min-h-0 flex-col gap-3">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-2">
        {LEVELS.map((level) => {
          const on = levelFilter.has(level)
          return (
            <button
              key={level}
              onClick={() => toggleLevel(level)}
              className={`rounded-full px-2.5 py-1 text-[11.5px] font-medium tabular-nums transition-colors ${
                on
                  ? 'bg-accent-light text-accent-text'
                  : 'bg-bg-tertiary/50 text-text-tertiary hover:text-text-secondary'
              }`}
            >
              {level}
            </button>
          )
        })}

        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter…"
          className="ml-1 min-w-0 flex-1 rounded-md border border-border bg-bg-primary px-2.5 py-1.5 text-[12.5px] text-text-primary outline-none placeholder:text-text-tertiary focus:border-accent"
        />

        <button
          onClick={() => setAutoScroll((v) => !v)}
          title="Auto-scroll"
          className={`flex items-center gap-1 rounded-md border border-border px-2 py-1.5 text-[12px] transition-colors ${
            autoScroll ? 'text-accent-text' : 'text-text-tertiary hover:text-text-secondary'
          }`}
        >
          <ArrowDownToLine size={14} />
        </button>
        <button
          onClick={copyAll}
          title="Copy visible"
          className="flex items-center gap-1 rounded-md border border-border px-2 py-1.5 text-[12px] text-text-secondary transition-colors hover:text-text-primary"
        >
          {copied ? <Check size={14} className="text-green-500" /> : <Copy size={14} />}
        </button>
        <button
          onClick={clear}
          title="Clear"
          className="flex items-center gap-1 rounded-md border border-border px-2 py-1.5 text-[12px] text-text-secondary transition-colors hover:text-red-500"
        >
          <Trash2 size={14} />
        </button>
      </div>

      {/* Counts */}
      <div className="flex items-center gap-3 text-[11.5px] tabular-nums text-text-tertiary">
        <span>{filtered.length} shown</span>
        <span>·</span>
        <span>{entries.length} total</span>
        {errorCount > 0 && (
          <>
            <span>·</span>
            <span className="text-amber-500">{errorCount} warn/error</span>
          </>
        )}
      </div>

      {/* Log */}
      <div
        ref={scrollRef}
        className="h-[52vh] min-h-[280px] overflow-auto rounded-lg border border-border bg-bg-primary p-2 font-mono text-[11.5px] leading-relaxed"
      >
        {filtered.length === 0 ? (
          <div className="grid h-full place-items-center text-text-tertiary">
            No log lines yet — trigger an Act command to see activity here.
          </div>
        ) : (
          filtered.map((e) => (
            <div
              key={e.seq}
              className="flex gap-2 whitespace-pre-wrap break-words border-b border-border/40 py-0.5 last:border-b-0"
            >
              <span className="shrink-0 text-text-tertiary">{formatTime(e.ts_ms)}</span>
              <span className={`w-10 shrink-0 font-semibold ${LEVEL_STYLES[e.level] ?? ''}`}>
                {e.level}
              </span>
              <span className="shrink-0 text-text-tertiary">{shortTarget(e.target)}</span>
              <span className="min-w-0 text-text-primary">{e.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
