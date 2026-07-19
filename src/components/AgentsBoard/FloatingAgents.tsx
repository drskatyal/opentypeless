import { AnimatePresence, motion } from 'framer-motion'
import { Bot } from 'lucide-react'
import { useState } from 'react'
import { useActTasks } from '../../hooks/useActTasks'
import { useAgentsWindowResize } from '../../hooks/useAgentsWindowResize'
import { useAppStore } from '../../stores/appStore'
import { CollapsedRail, EmptyState, OrchestrationList, PanelHeader, tallyStatuses } from './index'

/**
 * Contents of the dedicated always-on-top `agents` window (`index.html#agents`).
 * The OS window itself is the widget: it stays flush to the right edge, is a
 * thin tab while collapsed, and grows leftward into the full panel while the
 * user hovers or has pinned it (see `useAgentsWindowResize`). It only appears
 * when Act is enabled, and never takes focus.
 */
export function FloatingAgents() {
  const tasks = useActTasks()
  const actEnabled = useAppStore((s) => s.config.act_enabled)
  const [hovered, setHovered] = useState(false)
  const [pinned, setPinned] = useState(false)
  const open = hovered || pinned
  const counts = tallyStatuses(tasks)

  // Hook runs unconditionally (before any early return): it owns the window's
  // size/position/visibility. `visible = actEnabled` hides the window entirely
  // for users who don't use Act.
  useAgentsWindowResize(open, actEnabled)

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
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.12 }}
            className="flex h-full w-full flex-col overflow-hidden rounded-l-[16px] border border-r-0 border-border bg-bg-secondary/95 shadow-float backdrop-blur"
          >
            <PanelHeader
              total={tasks.length}
              counts={counts}
              pinned={pinned}
              onTogglePin={() => setPinned((p) => !p)}
            />
            <div className="flex-1 overflow-y-auto pt-2 pb-2.5">
              {tasks.length === 0 ? <EmptyState /> : <OrchestrationList tasks={tasks} />}
            </div>
          </motion.div>
        ) : (
          <motion.button
            key="tab"
            type="button"
            onClick={() => setPinned(true)}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.12 }}
            aria-label={`Agents${tasks.length ? ` — ${tasks.length} active` : ''}`}
            className="flex h-full w-full cursor-pointer flex-col items-center justify-center gap-1.5 rounded-l-[14px] border border-r-0 border-border bg-bg-secondary/85 shadow-md backdrop-blur"
          >
            {tasks.length === 0 ? (
              <span className="flex h-5 w-5 items-center justify-center text-text-tertiary">
                <Bot size={16} strokeWidth={2} />
              </span>
            ) : (
              <CollapsedRail tasks={tasks} />
            )}
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  )
}
