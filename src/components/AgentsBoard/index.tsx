import { AnimatePresence, motion } from 'framer-motion'
import { Check, Loader2, X } from 'lucide-react'
import { useActTasks, type ActTask } from '../../hooks/useActTasks'

/**
 * The live Agents board: while an Act command runs, each of its missions shows
 * as a card that moves Running → Done ✓ / Failed, so the user sees parallel
 * tasks check themselves off. A floating, non-modal stack (Hey Clicky-style);
 * hidden entirely when there are no tasks.
 */
export function AgentsBoard() {
  const tasks = useActTasks()
  if (tasks.length === 0) return null

  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-[9990] flex w-[300px] flex-col gap-2">
      <AnimatePresence initial={false}>
        {tasks.map((task) => (
          <TaskCard key={task.id} task={task} />
        ))}
      </AnimatePresence>
    </div>
  )
}

const RAIL: Record<ActTask['status'], string> = {
  running: 'bg-[#5aa0ff]',
  done: 'bg-[#54e0b0]',
  failed: 'bg-[#ff6b6b]',
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
      className="pointer-events-auto relative overflow-hidden rounded-[12px] border border-border bg-bg-secondary/95 px-3 py-2.5 shadow-float backdrop-blur"
    >
      <span className={`absolute inset-y-0 left-0 w-[3px] ${RAIL[task.status]}`} />

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
