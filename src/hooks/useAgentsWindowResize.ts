import { useEffect, useRef } from 'react'

/**
 * Drives the dedicated always-on-top "agents" window. It sits in the TOP-RIGHT
 * corner of the screen (a well-known notification spot, like macOS). Collapsed,
 * it is a thin vertical strip of small status orbs — one per agent — sized to the
 * agent count so it only ever covers a sliver of the corner (clicks land on the
 * app behind it everywhere else). Hovering expands it leftward into a card that
 * shows every agent's label + live step. Frontend-driven sizing, mirroring the
 * capsule (`useCapsuleResize`) — no Rust side needed.
 */

const MARGIN = 18
const ORB_STRIP_W = 46
const ORB_PITCH = 24 // vertical spacing per orb
const ORB_PAD = 16
const EXPANDED_W = 344

function clamp(n: number, lo: number, hi: number) {
  return Math.min(Math.max(n, lo), hi)
}

/** Thin orb strip: width fixed, height scales with the agent count. */
function collapsedSize(count: number) {
  return { width: ORB_STRIP_W, height: clamp(count * ORB_PITCH + ORB_PAD, 44, 460) }
}

/** Expanded card: wide enough for labels, height scales with the agent count. */
function expandedSize(count: number) {
  return { width: EXPANDED_W, height: clamp(count * 46 + 64, 96, 588) }
}

export function useAgentsWindowResize(
  open: boolean,
  visible: boolean,
  positionLocked = false,
  count = 1,
) {
  // Latest desired state, read by the single serialized applier below so rapid
  // hover/pin toggles can't interleave setSize/setPosition/show calls.
  const desired = useRef({ open, visible, positionLocked, count })
  desired.current = { open, visible, positionLocked, count }
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
          const { open: o, visible: v, positionLocked: locked, count: c } = desired.current
          const key = `${o}|${v}|${locked}|${c}`
          if (key === last) break
          last = key

          const size = o ? expandedSize(c) : collapsedSize(c)
          await win.setSize(new LogicalSize(size.width, size.height)).catch(() => {})
          // Once the user has dragged the widget somewhere, respect that spot —
          // only resize in place. Until then, anchor it top-right so the orb strip
          // and the expanded card hang from the same corner and never jump.
          if (!locked) {
            const monitor = await currentMonitor().catch(() => null)
            if (monitor) {
              const sw = monitor.size.width / monitor.scaleFactor
              const x = Math.round(sw - size.width - MARGIN)
              const y = MARGIN
              await win.setPosition(new LogicalPosition(x, y)).catch(() => {})
            }
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
  }, [open, visible, positionLocked, count])
}
