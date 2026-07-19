import { AnimatePresence, motion } from 'framer-motion'
import { Bot, Check, Loader2, Pin, Radio, X } from 'lucide-react'
import { useState } from 'react'
import { useActTasks, type ActTask } from '../../hooks/useActTasks'

/**
 * The live Agents dock: a persistent tab pinned to the right edge of the screen
 * that is ALWAYS present. Hover peeks the panel open; a click PINS it open so it
 * stays while you read. The panel header shows how many agents are running /
 * done / failed, and each agent (an Act mission) gets a card with its current
 * activity and final result.
 *
 * Non-modal: `pointer-events-none` on the outer container so it never blocks the
 * user's work; only the tab and panel themselves are interactive.
 */
export function AgentsBoard() {
  const tasks = useActTasks()
  const [hovered, setHovered] = useState(false)
  const [pinned, setPinned] = useState(false)
  const open = hovered || pinned

  const counts = tallyStatuses(tasks)

  return (
    <div className="pointer-events-none fixed top-1/2 right-0 z-[9990] -translate-y-1/2">
      <div
        className="pointer-events-auto relative flex justify-end"
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
      >
        {/* Persistent edge tab — always present. Fades out (keeping its edge
            footprint) while the panel is open. Click pins the panel open. */}
        <motion.button
          type="button"
          aria-hidden={open}
          aria-label={`Agents${tasks.length ? ` — ${tasks.length} active` : ''}`}
          onClick={() => setPinned(true)}
          onFocus={() => setHovered(true)}
          animate={{ opacity: open ? 0 : 1 }}
          transition={{ duration: 0.15 }}
          className="flex cursor-pointer flex-col items-center gap-1.5 rounded-l-[14px] border border-r-0 border-border bg-bg-secondary/80 px-2 py-2.5 shadow-md backdrop-blur"
        >
          {tasks.length === 0 ? (
            <span className="flex h-5 w-5 items-center justify-center text-text-tertiary">
              <Bot size={15} strokeWidth={2} />
            </span>
          ) : (
            <CollapsedRail tasks={tasks} />
          )}
        </motion.button>

        {/* Panel — full agent cards + counts header, springs in leftward. */}
        <AnimatePresence>
          {open && (
            <motion.div
              key="panel"
              initial={{ opacity: 0, x: 24, scale: 0.96 }}
              animate={{ opacity: 1, x: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24, scale: 0.96 }}
              transition={{ type: 'spring', stiffness: 420, damping: 34 }}
              style={{ transformOrigin: 'right center' }}
              className="absolute top-1/2 right-0 flex max-h-[80vh] w-[308px] -translate-y-1/2 flex-col overflow-hidden rounded-[16px] border border-border bg-bg-secondary/95 shadow-float backdrop-blur"
            >
              <PanelHeader
                total={tasks.length}
                counts={counts}
                pinned={pinned}
                onTogglePin={() => setPinned((p) => !p)}
              />
              <div className="overflow-y-auto pt-2 pb-2.5">
                {tasks.length === 0 ? <EmptyState /> : <OrchestrationList tasks={tasks} />}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  )
}

export interface StatusTally {
  running: number
  done: number
  failed: number
}

export function tallyStatuses(tasks: ActTask[]): StatusTally {
  return tasks.reduce(
    (acc, t) => {
      acc[t.status] += 1
      return acc
    },
    { running: 0, done: 0, failed: 0 } as StatusTally,
  )
}

export function PanelHeader({
  total,
  counts,
  pinned,
  onTogglePin,
}: {
  total: number
  counts: StatusTally
  pinned: boolean
  onTogglePin: () => void
}) {
  const running = counts.running > 0
  const parts: string[] = []
  if (counts.running) parts.push(`${counts.running} running`)
  if (counts.done) parts.push(`${counts.done} done`)
  if (counts.failed) parts.push(`${counts.failed} failed`)
  const subtitle =
    total === 0
      ? 'No missions running'
      : `${total} mission${total === 1 ? '' : 's'} · ${parts.join(' · ')}`

  return (
    <div className="flex items-center justify-between gap-2 border-b border-border/70 px-3.5 py-2.5">
      <div className="flex min-w-0 items-center gap-2.5">
        {/* The Conductor node — orchestrator of the missions below. Its orb
            pulses and the waveform animates whenever any mission is running. */}
        <span className="relative flex h-[28px] w-[28px] flex-none items-center justify-center rounded-[9px] border border-accent/40 bg-accent/12 text-accent">
          <Radio size={15} strokeWidth={2.2} />
          {running && (
            <motion.span
              className="absolute inset-0 rounded-[9px] ring-1 ring-accent"
              animate={{ scale: [1, 1.5], opacity: [0.5, 0] }}
              transition={{ duration: 2, repeat: Infinity, ease: 'easeOut' }}
            />
          )}
        </span>
        <div className="min-w-0 leading-tight">
          <p className="text-[13px] font-semibold text-text-primary">Conductor</p>
          <p className="truncate text-[11px] text-text-secondary">{subtitle}</p>
        </div>
      </div>
      <div className="flex flex-none items-center gap-2.5">
        <span className={`agents-wave ${running ? 'live' : ''}`} aria-hidden="true">
          <i></i>
          <i></i>
          <i></i>
          <i></i>
          <i></i>
        </span>
        <button
          type="button"
          onClick={onTogglePin}
          aria-pressed={pinned}
          aria-label={pinned ? 'Unpin agents panel' : 'Keep agents panel open'}
          className={`flex h-6 w-6 flex-none items-center justify-center rounded-[7px] border transition-colors ${
            pinned
              ? 'border-accent/40 bg-accent/12 text-accent-text'
              : 'border-transparent text-text-tertiary hover:bg-bg-tertiary hover:text-text-secondary'
          }`}
        >
          <Pin size={13} strokeWidth={pinned ? 2.4 : 2} />
        </button>
      </div>
    </div>
  )
}

/**
 * The mission list as an orchestration tree: a vertical spine drops from the
 * Conductor, a luminous segment flows down it while anything runs, and each
 * mission taps off it with a status-colored node. Reads as a control graph,
 * not a flat list.
 */
export function OrchestrationList({ tasks }: { tasks: ActTask[] }) {
  const running = tasks.some((t) => t.status === 'running')
  return (
    <div className="relative pl-[26px]">
      <span
        className={`absolute top-1.5 bottom-4 left-[10px] w-[2px] ${running ? 'agents-spine flowing' : 'agents-spine'}`}
        aria-hidden="true"
      />
      <div className="flex flex-col gap-2.5">
        <AnimatePresence initial={false}>
          {tasks.map((task) => (
            <div key={task.id} className="relative">
              <span
                className={`absolute top-[19px] -left-[20px] z-10 h-[9px] w-[9px] rounded-full ring-[3px] ring-bg-secondary ${STATUS[task.status].dot}`}
                aria-hidden="true"
              />
              <TaskCard task={task} />
            </div>
          ))}
        </AnimatePresence>
      </div>
    </div>
  )
}

export function EmptyState() {
  return (
    <div className="flex flex-col items-center gap-1.5 px-4 py-8 text-center">
      <span className="flex h-9 w-9 items-center justify-center rounded-full bg-bg-tertiary text-text-tertiary">
        <Bot size={18} strokeWidth={1.8} />
      </span>
      <p className="text-[12.5px] font-medium text-text-secondary">No agents yet</p>
      <p className="text-[11px] leading-relaxed text-text-tertiary">
        Fire an Act command and each task shows up here with live status.
      </p>
    </div>
  )
}

/** How many indicators to show before folding the rest into a "+N" chip. */
const MAX_DOTS = 7

export function CollapsedRail({ tasks }: { tasks: ActTask[] }) {
  const overflowing = tasks.length > MAX_DOTS
  const visible = overflowing ? tasks.slice(0, MAX_DOTS - 1) : tasks
  const hidden = tasks.length - visible.length

  return (
    <>
      <AnimatePresence initial={false}>
        {visible.map((task) => (
          <Indicator key={task.id} task={task} />
        ))}
      </AnimatePresence>
      {hidden > 0 && <OverflowChip count={hidden} tasks={tasks.slice(visible.length)} />}
    </>
  )
}

/** Per-status token styling — one source of truth so the rail, cards, pills,
 *  and stripes all speak the same language: aqua accent = running, success =
 *  done, error = failed. No hardcoded hexes; everything rides the theme tokens. */
const STATUS: Record<
  ActTask['status'],
  { dot: string; text: string; soft: string; border: string; label: string }
> = {
  running: {
    dot: 'bg-accent',
    text: 'text-accent',
    soft: 'bg-accent/12',
    border: 'border-accent/35',
    label: 'RUNNING',
  },
  done: {
    dot: 'bg-success',
    text: 'text-success',
    soft: 'bg-success/14',
    border: 'border-success/30',
    label: 'DONE',
  },
  failed: {
    dot: 'bg-error',
    text: 'text-error',
    soft: 'bg-error/12',
    border: 'border-error/30',
    label: 'FAILED',
  },
}

/** One 16px status indicator in the collapsed rail. */
function Indicator({ task }: { task: ActTask }) {
  return (
    <motion.div
      layout
      initial={{ opacity: 0, scale: 0.4 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.4 }}
      transition={{ type: 'spring', stiffness: 500, damping: 32 }}
      className="relative flex h-4 w-4 flex-none items-center justify-center"
    >
      {/* Pulsing halo while running */}
      {task.status === 'running' && (
        <motion.span
          className="absolute inset-0 rounded-full bg-accent"
          animate={{ scale: [1, 1.85], opacity: [0.5, 0] }}
          transition={{ duration: 1.4, repeat: Infinity, ease: 'easeOut' }}
        />
      )}
      {/* The dot itself. `key={status}` remounts on a status flip so the
          one-shot settle/scale plays when a task lands on done/failed. */}
      <motion.span
        key={task.status}
        initial={{ scale: task.status === 'running' ? 1 : 0.4 }}
        animate={{ scale: task.status === 'running' ? 1 : [0.4, 1.35, 1] }}
        transition={{ duration: 0.28, ease: 'easeOut' }}
        className={`relative flex h-4 w-4 items-center justify-center rounded-full text-white shadow-sm ${STATUS[task.status].dot}`}
      >
        {task.status === 'done' && <Check size={10} strokeWidth={3.2} />}
        {task.status === 'failed' && <X size={10} strokeWidth={3.2} />}
      </motion.span>
    </motion.div>
  )
}

/** "+N" indicator that summarizes the tasks beyond the visible cap. */
function OverflowChip({ count, tasks }: { count: number; tasks: ActTask[] }) {
  const anyRunning = tasks.some((t) => t.status === 'running')
  const anyFailed = tasks.some((t) => t.status === 'failed')
  const tint = anyRunning ? 'text-accent' : anyFailed ? 'text-error' : 'text-success'
  return (
    <motion.div
      layout
      initial={{ opacity: 0, scale: 0.4 }}
      animate={{ opacity: 1, scale: 1 }}
      className={`flex h-4 min-w-4 flex-none items-center justify-center rounded-full bg-bg-tertiary px-1 text-[9px] font-semibold tabular-nums ${tint}`}
    >
      +{count}
    </motion.div>
  )
}

export function TaskCard({ task }: { task: ActTask }) {
  const running = task.status === 'running'
  const line = running ? task.detail : task.summary

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 6, scale: 0.98 }}
      transition={{ type: 'spring', stiffness: 420, damping: 34 }}
      className="pointer-events-auto relative flex-none overflow-hidden rounded-[13px] border border-border bg-bg-elevated px-3 py-2.5 shadow-sm"
    >
      <span className={`absolute inset-y-0 left-0 w-[3px] ${STATUS[task.status].dot}`} />

      <div className="flex items-start gap-2.5 pl-1.5">
        <StatusIcon status={task.status} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <p className="truncate text-[13px] font-semibold text-text-primary">{task.label}</p>
            <StatusBadge status={task.status} />
          </div>
          {line && (
            <p
              className={`mt-1 line-clamp-2 leading-relaxed ${
                running
                  ? 'font-mono text-[11px] text-text-secondary'
                  : 'text-[11.5px] text-text-secondary'
              }`}
            >
              {running && <span className="mr-1 text-accent">›</span>}
              {line}
            </p>
          )}
        </div>
      </div>

      {running && (
        <div className="mt-2.5 ml-1.5 h-[4px] overflow-hidden rounded-full bg-bg-tertiary">
          <motion.div
            className="h-full w-1/3 rounded-full bg-gradient-to-r from-accent-hover to-accent"
            animate={{ x: ['-120%', '340%'] }}
            transition={{ duration: 1.3, repeat: Infinity, ease: 'easeInOut' }}
          />
        </div>
      )}
    </motion.div>
  )
}

function StatusIcon({ status }: { status: ActTask['status'] }) {
  const s = STATUS[status]
  return (
    <span
      className={`mt-0.5 flex h-[24px] w-[24px] flex-none items-center justify-center rounded-[8px] ${s.soft} ${s.text}`}
    >
      {status === 'running' && <Loader2 size={13} className="animate-spin" strokeWidth={2.4} />}
      {status === 'done' && <Check size={13} strokeWidth={2.8} />}
      {status === 'failed' && <X size={13} strokeWidth={2.6} />}
    </span>
  )
}

function StatusBadge({ status }: { status: ActTask['status'] }) {
  const s = STATUS[status]
  return (
    <span
      className={`flex-none rounded-full border px-2 py-0.5 text-[9.5px] font-semibold tracking-[0.04em] ${s.text} ${s.soft} ${s.border}`}
    >
      {s.label}
    </span>
  )
}
