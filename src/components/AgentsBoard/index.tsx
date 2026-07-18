import { AnimatePresence, motion } from 'framer-motion'
import { Check, Loader2, X } from 'lucide-react'
import { useState } from 'react'
import { useActTasks, type ActTask } from '../../hooks/useActTasks'

/**
 * The live Agents rail: while an Act command runs, each of its missions shows
 * as a small status indicator in a compact vertical strip pinned to the right
 * edge of the screen (running → done ✓ / failed ✗), so the user can fire several
 * tasks and watch each check itself off at a glance.
 *
 * Collapsed, it's a click-through strip of dots. Hovering expands it leftward
 * into the full task cards (icon, label, progress line, status badge). It shows
 * only the CURRENT command's missions — the board clears per command.
 * Non-modal: `pointer-events-none` on the container so it never blocks the
 * user's work; only the rail itself is interactive.
 */
export function AgentsBoard() {
  const tasks = useActTasks()
  const [expanded, setExpanded] = useState(false)
  if (tasks.length === 0) return null

  return (
    <div className="pointer-events-none fixed top-1/2 right-0 z-[9990] -translate-y-1/2">
      <div
        className="pointer-events-auto relative flex justify-end"
        onMouseEnter={() => setExpanded(true)}
        onMouseLeave={() => setExpanded(false)}
        onFocus={() => setExpanded(true)}
        onBlur={() => setExpanded(false)}
      >
        {/* Collapsed rail — the persistent hover surface. Fades out (but keeps
            its footprint on the edge) while the expanded panel is shown. */}
        <motion.div
          aria-hidden={expanded}
          animate={{ opacity: expanded ? 0 : 1 }}
          transition={{ duration: 0.15 }}
          className="flex flex-col items-center gap-1.5 rounded-l-[14px] border border-r-0 border-border bg-bg-secondary/80 px-2 py-2.5 shadow-md backdrop-blur"
        >
          <CollapsedRail tasks={tasks} />
        </motion.div>

        {/* Expanded panel — full task cards, springs in leftward on hover. */}
        <AnimatePresence>
          {expanded && (
            <motion.div
              key="panel"
              initial={{ opacity: 0, x: 24, scale: 0.96 }}
              animate={{ opacity: 1, x: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24, scale: 0.96 }}
              transition={{ type: 'spring', stiffness: 420, damping: 34 }}
              style={{ transformOrigin: 'right center' }}
              className="absolute top-1/2 right-0 flex max-h-[78vh] w-[300px] -translate-y-1/2 flex-col gap-2 overflow-y-auto pr-1.5 pl-1"
            >
              {tasks.map((task) => (
                <TaskCard key={task.id} task={task} />
              ))}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  )
}

/** How many indicators to show before folding the rest into a "+N" chip. */
const MAX_DOTS = 7

function CollapsedRail({ tasks }: { tasks: ActTask[] }) {
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

const DOT: Record<ActTask['status'], string> = {
  running: 'bg-[#5aa0ff]',
  done: 'bg-[#54e0b0]',
  failed: 'bg-[#ff6b6b]',
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
          className="absolute inset-0 rounded-full bg-[#5aa0ff]"
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
        className={`relative flex h-4 w-4 items-center justify-center rounded-full text-white shadow-sm ${DOT[task.status]}`}
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
  const tint = anyRunning ? 'text-[#5aa0ff]' : anyFailed ? 'text-[#ff6b6b]' : 'text-[#54e0b0]'
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

function TaskCard({ task }: { task: ActTask }) {
  const line = task.status === 'running' ? task.detail : task.summary

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 6, scale: 0.98 }}
      transition={{ type: 'spring', stiffness: 420, damping: 34 }}
      className="pointer-events-auto relative flex-none overflow-hidden rounded-[12px] border border-border bg-bg-secondary/95 px-3 py-2.5 shadow-float backdrop-blur"
    >
      <span className={`absolute inset-y-0 left-0 w-[3px] ${DOT[task.status]}`} />

      <div className="flex items-start gap-2.5 pl-1.5">
        <StatusIcon status={task.status} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <p className="truncate text-[13px] font-medium text-text-primary">{task.label}</p>
            <StatusBadge status={task.status} />
          </div>
          {line && (
            <p className="mt-0.5 line-clamp-2 text-[11.5px] leading-relaxed text-text-secondary">
              {line}
            </p>
          )}
        </div>
      </div>

      {task.status === 'running' && (
        <div className="mt-2 ml-1.5 h-[3px] overflow-hidden rounded-full bg-white/8">
          <motion.div
            className="h-full w-1/3 rounded-full bg-[#5aa0ff]"
            animate={{ x: ['-120%', '340%'] }}
            transition={{ duration: 1.3, repeat: Infinity, ease: 'easeInOut' }}
          />
        </div>
      )}
    </motion.div>
  )
}

function StatusIcon({ status }: { status: ActTask['status'] }) {
  if (status === 'running') {
    return (
      <span className="mt-0.5 flex h-[22px] w-[22px] flex-none items-center justify-center rounded-[7px] bg-[#5aa0ff]/12 text-[#5aa0ff]">
        <Loader2 size={13} className="animate-spin" strokeWidth={2.4} />
      </span>
    )
  }
  if (status === 'done') {
    return (
      <span className="mt-0.5 flex h-[22px] w-[22px] flex-none items-center justify-center rounded-[7px] bg-[#54e0b0]/14 text-[#54e0b0]">
        <Check size={13} strokeWidth={2.8} />
      </span>
    )
  }
  return (
    <span className="mt-0.5 flex h-[22px] w-[22px] flex-none items-center justify-center rounded-[7px] bg-[#ff6b6b]/12 text-[#ff6b6b]">
      <X size={13} strokeWidth={2.6} />
    </span>
  )
}

function StatusBadge({ status }: { status: ActTask['status'] }) {
  const map = {
    running: { label: 'RUNNING', cls: 'text-[#5aa0ff] bg-[#5aa0ff]/12 border-[#5aa0ff]/35' },
    done: { label: 'DONE', cls: 'text-[#54e0b0] bg-[#54e0b0]/12 border-[#54e0b0]/32' },
    failed: { label: 'FAILED', cls: 'text-[#ff6b6b] bg-[#ff6b6b]/12 border-[#ff6b6b]/32' },
  }[status]
  return (
    <span
      className={`flex-none rounded-full border px-2 py-0.5 text-[9.5px] font-semibold tracking-[0.03em] ${map.cls}`}
    >
      {map.label}
    </span>
  )
}
