import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { useCallback, useEffect, useState } from 'react'
import { useActTasks, type ActTask, type ActTaskStatus } from '../../hooks/useActTasks'
import { useAgentsWindowResize } from '../../hooks/useAgentsWindowResize'
import { useAppStore } from '../../stores/appStore'

/**
 * Per-status orb styling. Each agent is a small solid orb: dim while queued,
 * amber and pulsing while working, green when done, red on error. No grey box —
 * just a column of orbs in the corner that you can glance at.
 */
const ORB: Record<ActTaskStatus, { dot: string; glow: string; label: string }> = {
  queued: { dot: 'bg-text-tertiary/45', glow: '', label: 'Queued' },
  running: {
    dot: 'bg-amber-400',
    glow: 'shadow-[0_0_10px_2px_rgba(251,191,36,0.6)]',
    label: 'Working',
  },
  done: {
    dot: 'bg-emerald-400',
    glow: 'shadow-[0_0_8px_1px_rgba(52,211,153,0.5)]',
    label: 'Done',
  },
  failed: {
    dot: 'bg-red-400',
    glow: 'shadow-[0_0_9px_2px_rgba(248,113,113,0.6)]',
    label: 'Error',
  },
}

/** Start an OS-level window drag from a pointer-down that isn't on a control. */
function useWindowDrag(onMoved: () => void) {
  return useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0) return
      const target = e.target as HTMLElement
      if (target.closest('button, a, input, [data-no-drag]')) return
      onMoved()
      void import('@tauri-apps/api/window')
        .then(({ getCurrentWindow }) => getCurrentWindow().startDragging())
        .catch(() => {})
    },
    [onMoved],
  )
}

/** A single solid orb, pulsing while its agent is working. */
function Orb({ status, size = 12 }: { status: ActTaskStatus; size?: number }) {
  const reduced = useReducedMotion()
  const s = ORB[status]
  return (
    <span
      className="relative flex flex-none items-center justify-center"
      style={{ width: size, height: size }}
    >
      {status === 'running' && !reduced && (
        <motion.span
          aria-hidden="true"
          className="absolute inset-0 rounded-full bg-amber-400"
          animate={{ scale: [1, 1.9], opacity: [0.55, 0] }}
          transition={{ duration: 1.4, repeat: Infinity, ease: 'easeOut' }}
        />
      )}
      <span className={`h-full w-full rounded-full ${s.dot} ${s.glow}`} />
    </span>
  )
}

/** Collapsed view: a thin vertical column of orbs, one per agent. */
function OrbColumn({ tasks, present }: { tasks: ActTask[]; present: boolean }) {
  return (
    <motion.div
      key="orbs"
      initial={{ opacity: 0, x: 8 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 8 }}
      transition={{ type: 'spring', stiffness: 460, damping: 32 }}
      className="flex h-full w-full flex-col items-center justify-start gap-3 py-2.5"
    >
      {tasks.length === 0 ? (
        // Idle: a single dim orb so the user knows the HUD is there.
        <span
          className={`h-2.5 w-2.5 rounded-full ${present ? 'bg-text-tertiary/45' : 'bg-text-tertiary/25'}`}
        />
      ) : (
        <AnimatePresence initial={false}>
          {tasks.map((t) => (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, scale: 0.4 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.4 }}
              transition={{ type: 'spring', stiffness: 520, damping: 30 }}
            >
              <Orb status={t.status} />
            </motion.div>
          ))}
        </AnimatePresence>
      )}
    </motion.div>
  )
}

/** Expanded view: one row per agent — orb + label + live step. */
function ExpandedList({ tasks }: { tasks: ActTask[] }) {
  const running = tasks.filter((t) => t.status === 'running').length
  const done = tasks.filter((t) => t.status === 'done').length
  const failed = tasks.filter((t) => t.status === 'failed').length
  const parts = [
    running ? `${running} working` : '',
    done ? `${done} done` : '',
    failed ? `${failed} failed` : '',
  ].filter(Boolean)

  return (
    <motion.div
      key="list"
      initial={{ opacity: 0, scale: 0.97, x: 8 }}
      animate={{ opacity: 1, scale: 1, x: 0 }}
      exit={{ opacity: 0, scale: 0.97, x: 8 }}
      transition={{ type: 'spring', stiffness: 460, damping: 34 }}
      style={{ transformOrigin: 'top right' }}
      className="flex h-full w-full flex-col overflow-hidden rounded-[14px] bg-bg-secondary/95 shadow-float ring-1 ring-accent/15 backdrop-blur"
    >
      <div className="flex items-center justify-between border-b border-border/60 px-3 py-2">
        <span className="text-[12px] font-semibold text-text-primary">
          {tasks.length} agent{tasks.length === 1 ? '' : 's'}
        </span>
        <span className="truncate text-[10.5px] text-text-tertiary">
          {parts.join(' · ') || 'idle'}
        </span>
      </div>
      <div className="flex-1 overflow-y-auto py-1.5">
        {tasks.map((t) => (
          <div key={t.id} className="flex items-start gap-2.5 px-3 py-1.5">
            <span className="mt-[3px]">
              <Orb status={t.status} size={11} />
            </span>
            <div className="min-w-0 flex-1 leading-tight">
              <p className="truncate text-[12px] font-medium text-text-primary">{t.label}</p>
              <p className="truncate text-[10.5px] text-text-secondary">
                {t.status === 'failed' && t.summary
                  ? t.summary
                  : t.detail || t.summary || ORB[t.status].label}
              </p>
            </div>
          </div>
        ))}
      </div>
    </motion.div>
  )
}

/**
 * Contents of the dedicated always-on-top `agents` window (`index.html#agents`).
 * The OS window is a thin vertical strip of status orbs top-right — one orb per
 * agent, colour-coded and pulsing — that expands into a labelled card on hover.
 * It only exists when Act is enabled, and never takes focus.
 */
export function FloatingAgents() {
  const tasks = useActTasks()
  const actEnabled = useAppStore((s) => s.config.act_enabled)
  const [hovered, setHovered] = useState(false)
  const [pinned, setPinned] = useState(false)

  // Keep the strip present briefly after the last agent clears so it doesn't
  // flicker away between two back-to-back commands.
  const hasTasks = tasks.length > 0
  const [recentlyActive, setRecentlyActive] = useState(false)
  useEffect(() => {
    if (hasTasks) {
      setRecentlyActive(true)
      return
    }
    const timer = setTimeout(() => setRecentlyActive(false), 1800)
    return () => clearTimeout(timer)
  }, [hasTasks])

  const [moved, setMoved] = useState(false)
  const startDrag = useWindowDrag(() => setMoved(true))

  const present = hasTasks || recentlyActive
  const open = (hovered || pinned) && present
  const count = Math.max(tasks.length, 1)

  useAgentsWindowResize(open, actEnabled, moved, count)

  if (!actEnabled) return null

  return (
    <div
      className="flex h-screen w-screen items-start justify-end overflow-hidden"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onPointerDown={startDrag}
      onClick={() => present && setPinned((p) => !p)}
    >
      <AnimatePresence mode="wait" initial={false}>
        {open ? <ExpandedList tasks={tasks} /> : <OrbColumn tasks={tasks} present={present} />}
      </AnimatePresence>
    </div>
  )
}
