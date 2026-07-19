import { AnimatePresence, motion } from 'framer-motion'
import { Radio } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useActTasks } from '../../hooks/useActTasks'
import { useAgentsWindowResize } from '../../hooks/useAgentsWindowResize'
import { useAppStore } from '../../stores/appStore'
import { EmptyState, OrchestrationList, PanelHeader, tallyStatuses } from './index'

/**
 * Start an OS-level window drag from a pointer-down on a handle area. Ignores
 * presses that land on a button/interactive control so clicks (pin, collapse)
 * still register. Once a drag begins the widget's position is locked so the
 * resize hook stops snapping it back to the top-right anchor.
 */
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

/**
 * Contents of the dedicated always-on-top `agents` window (`index.html#agents`).
 * The OS window itself is the widget: it lives in the TOP-RIGHT corner, on top of
 * every other app, and AUTO-OPENS into the full panel the moment Act starts a
 * mission — so the user actually sees what the agents are doing without hunting
 * for a hidden edge tab. When idle it collapses to a small pill. It only exists
 * when Act is enabled, and never takes focus.
 */
export function FloatingAgents() {
  const tasks = useActTasks()
  const actEnabled = useAppStore((s) => s.config.act_enabled)
  const [hovered, setHovered] = useState(false)
  const [pinned, setPinned] = useState(false)
  // Auto-open whenever there are missions to show — the widget pops open on its
  // own when Act works, and falls back to the pill only when everything is idle.
  // A grace period keeps it open briefly after the last mission clears so the
  // panel doesn't flicker closed→open between two back-to-back commands (each new
  // command momentarily empties the task list before the next mission spawns).
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

  // Manual collapse: the user can fold the panel back to the pill even while Act
  // is working. It stays collapsed until they reopen it or the run fully clears.
  const [userCollapsed, setUserCollapsed] = useState(false)
  useEffect(() => {
    if (!hasTasks) setUserCollapsed(false)
  }, [hasTasks])

  // Once the widget is dragged, lock its position so the resize hook stops
  // re-anchoring it to the top-right corner.
  const [moved, setMoved] = useState(false)
  const startDrag = useWindowDrag(() => setMoved(true))

  const open = !userCollapsed && (hovered || pinned || hasTasks || recentlyActive)
  const counts = tallyStatuses(tasks)

  useAgentsWindowResize(open, actEnabled, moved)

  if (!actEnabled) return null

  return (
    <div
      className="h-screen w-screen overflow-hidden"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <AnimatePresence mode="wait" initial={false}>
        {open ? (
          <motion.div
            key="panel"
            initial={{ opacity: 0, scale: 0.96, y: -6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.96, y: -6 }}
            transition={{ type: 'spring', stiffness: 460, damping: 34 }}
            style={{ transformOrigin: 'top right' }}
            onPointerDown={startDrag}
            className="flex h-full w-full flex-col overflow-hidden rounded-[16px] bg-bg-secondary/95 shadow-float ring-1 ring-accent/15 backdrop-blur"
          >
            <PanelHeader
              total={tasks.length}
              counts={counts}
              pinned={pinned}
              onTogglePin={() => setPinned((p) => !p)}
              onCollapse={() => {
                setUserCollapsed(true)
                setPinned(false)
                setHovered(false)
              }}
            />
            <div className="flex-1 overflow-y-auto pt-2 pb-2.5">
              {tasks.length === 0 ? <EmptyState /> : <OrchestrationList tasks={tasks} />}
            </div>
          </motion.div>
        ) : (
          <motion.div
            key="pill"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={{ type: 'spring', stiffness: 460, damping: 30 }}
            onPointerDown={startDrag}
            className="flex h-full w-full items-center gap-2 rounded-full bg-bg-secondary/90 pr-2 pl-2.5 shadow-lg ring-1 ring-accent/15 backdrop-blur"
          >
            <button
              type="button"
              onClick={() => {
                setUserCollapsed(false)
                setPinned(true)
              }}
              aria-label="Open agents panel"
              className="flex flex-1 cursor-pointer items-center gap-2 bg-transparent"
            >
              <span className="flex h-7 w-7 flex-none items-center justify-center rounded-full border border-accent/40 bg-accent/12 text-accent">
                <Radio size={14} strokeWidth={2.2} />
              </span>
              <span className="text-[12.5px] font-semibold text-text-primary">Agents</span>
              <span className="ml-auto text-[11px] text-text-tertiary">
                {hasTasks ? `${counts.running} live` : 'idle'}
              </span>
            </button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
