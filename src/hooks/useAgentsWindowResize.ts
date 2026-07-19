import { useEffect, useRef } from 'react'

/**
 * Drives the dedicated always-on-top "agents" window: keeps it flush to the
 * right edge of the screen (vertically centred), grows it leftward when the
 * panel is open, and shows/hides it with `visible`. Mirrors the capsule's
 * frontend-driven sizing (`useCapsuleResize`) — no Rust side needed.
 *
 * The window is kept small while collapsed so it only covers a thin edge strip
 * (clicks land on whatever app is behind it everywhere else); it grows only
 * while the user is actively hovering/pinning it open.
 */

const COLLAPSED = { width: 56, height: 148 }
const EXPANDED = { width: 336, height: 544 }

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
            const sh = monitor.size.height / monitor.scaleFactor
            const x = Math.round(sw - size.width) // flush to the right edge
            const y = Math.round(sh / 2 - size.height / 2)
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
