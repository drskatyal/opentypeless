import { AnimatePresence, motion } from 'framer-motion'
import { Radio } from 'lucide-react'
import { useState } from 'react'
import { useActTasks } from '../../hooks/useActTasks'
import { useAgentsWindowResize } from '../../hooks/useAgentsWindowResize'
import { useAppStore } from '../../stores/appStore'
import { EmptyState, OrchestrationList, PanelHeader, tallyStatuses } from './index'

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
  const hasTasks = tasks.length > 0
  const open = hovered || pinned || hasTasks
  const counts = tallyStatuses(tasks)

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
            initial={{ opacity: 0, scale: 0.96, y: -6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.96, y: -6 }}
            transition={{ type: 'spring', stiffness: 460, damping: 34 }}
            style={{ transformOrigin: 'top right' }}
            className="flex h-full w-full flex-col overflow-hidden rounded-[16px] border border-border bg-bg-secondary/95 shadow-float backdrop-blur"
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
            key="pill"
            type="button"
            onClick={() => setPinned(true)}
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            transition={{ type: 'spring', stiffness: 460, damping: 30 }}
            aria-label="Agents"
            className="flex h-full w-full cursor-pointer items-center gap-2 rounded-full border border-border bg-bg-secondary/90 pr-3.5 pl-2.5 shadow-lg backdrop-blur"
          >
            <span className="flex h-7 w-7 flex-none items-center justify-center rounded-full border border-accent/40 bg-accent/12 text-accent">
              <Radio size={14} strokeWidth={2.2} />
            </span>
            <span className="text-[12.5px] font-semibold text-text-primary">Agents</span>
            <span className="ml-auto text-[11px] text-text-tertiary">idle</span>
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  )
}
