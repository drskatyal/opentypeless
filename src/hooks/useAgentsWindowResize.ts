import { useEffect, useRef } from 'react'

/**
 * Drives the dedicated always-on-top "agents" window. It sits in the TOP-RIGHT
 * corner of the screen (a well-known notification spot, like macOS), auto-opens
 * into the full panel whenever Act is working, and collapses to a small pill when
 * idle. The whole point of this window is to be SEEN on top of other apps while
 * the main app lives in the tray — so it favours visibility over hiding.
 *
 * Collapsed it's a small pill that still covers only a corner (clicks land on the
 * app behind it everywhere else); it grows down-and-left into the card stack when
 * open. Frontend-driven sizing, mirroring the capsule (`useCapsuleResize`).
 */

const MARGIN = 18
const COLLAPSED = { width: 132, height: 44 }
const EXPANDED = { width: 348, height: 560 }

export function useAgentsWindowResize(open: boolean, visible: boolean) {
  // Latest desired state, read by the single serialized applier below so rapid
  // hover/pin toggles can't interleave setSize/setPosition/show calls.
  const desired = useRef({ open, visible })
  desired.current = { open, visible }
  const running = useRef(false)

  useEffect(() => {
    let cancelled = false

    const apply = async () => {
      if (running.current) return
      running.current = true
      try {
        const { getCurrentWindow, LogicalSize, LogicalPosition, currentMonitor } =
          await import('@tauri-apps/api/window')
        const win = getCurrentWindow()
        // Never let the widget steal focus (it floats during dictation / Act).
        await win.setFocusable(false).catch(() => {})

        // Apply until the DOM-desired state stops changing (coalesces bursts).
        let last = ''
        for (let i = 0; i < 6; i += 1) {
          if (cancelled) break
          const { open: o, visible: v } = desired.current
          const key = `${o}|${v}`
          if (key === last) break
          last = key

          const size = o ? EXPANDED : COLLAPSED
          await win.setSize(new LogicalSize(size.width, size.height)).catch(() => {})
          const monitor = await currentMonitor().catch(() => null)
          if (monitor) {
            const sw = monitor.size.width / monitor.scaleFactor
            // Top-right corner with a margin — the collapsed pill and the open
            // card stack both hang from the same top-right anchor, so the widget
            // never jumps around as it grows.
            const x = Math.round(sw - size.width - MARGIN)
            const y = MARGIN
            await win.setPosition(new LogicalPosition(x, y)).catch(() => {})
          }
          if (v) await win.show().catch(() => {})
          else await win.hide().catch(() => {})
        }
      } finally {
        running.current = false
      }
    }

    void apply()
    return () => {
      cancelled = true
    }
  }, [open, visible])
}
