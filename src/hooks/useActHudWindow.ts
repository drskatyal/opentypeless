import { useEffect, useRef } from 'react'

/**
 * Drives the dedicated always-on-top `acthud` window (`index.html#acthud`), which
 * hosts ALL of Act's interactive UI — the confirm dialog, the ask_user picker, the
 * talk-back answer card, the live step indicator and the abort control. Because the
 * main app lives minimised in the tray, these must render on a floating overlay
 * (like the capsule and agents windows) or the user could never see or answer them.
 *
 * The window is stretched to cover the whole monitor so that `ActHud`'s fixed cards
 * land in the right screen spots (bottom-left pill / say card, centred modal), while
 * staying transparent everywhere else.
 *
 * Focus is the crux:
 *  - Idle (Act enabled, no prompt — just a step pill or a talk-back toast): the
 *    window is CLICK-THROUGH (`setIgnoreCursorEvents(true)`) and NON-FOCUSABLE, so it
 *    never steals focus from the app the user is working in and never blocks clicks
 *    on the desktop behind it.
 *  - Prompting (a confirm or ask_user is active): the window becomes focusable and
 *    captures the cursor so the buttons are clickable and the 1-9 / Esc keys reach
 *    it, and we pull focus so the number keys work immediately.
 *
 * Frontend-driven, mirroring `useAgentsWindowResize` / `useCapsuleResize` — no Rust
 * side needed. A single serialised applier coalesces rapid enable/prompt toggles so
 * setSize/setPosition/show/focus calls can't interleave.
 */
export function useActHudWindow(enabled: boolean, promptActive: boolean) {
  // Latest desired state, read by the serialised applier below.
  const desired = useRef({ enabled, promptActive })
  desired.current = { enabled, promptActive }
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

        // Apply until the DOM-desired state stops changing (coalesces bursts).
        let last = ''
        for (let i = 0; i < 6; i += 1) {
          if (cancelled) break
          const { enabled: en, promptActive: active } = desired.current
          const key = `${en}|${active}`
          if (key === last) break
          last = key

          if (!en) {
            // Act off: nothing to show — click-through, non-focusable, hidden.
            await win.setFocusable(false).catch(() => {})
            await win.setIgnoreCursorEvents(true).catch(() => {})
            await win.hide().catch(() => {})
            continue
          }

          // Cover the full primary monitor so the bottom-left pill/toast and the
          // centred confirm/ask modal each land where their fixed CSS expects.
          const monitor = await currentMonitor().catch(() => null)
          if (monitor) {
            const sw = monitor.size.width / monitor.scaleFactor
            const sh = monitor.size.height / monitor.scaleFactor
            await win.setSize(new LogicalSize(sw, sh)).catch(() => {})
            await win.setPosition(new LogicalPosition(0, 0)).catch(() => {})
          }

          // Interactive ONLY while a prompt is up. Set cursor pass-through and
          // focusability BEFORE showing so an idle overlay never grabs a click or
          // focus in the frame it appears.
          await win.setIgnoreCursorEvents(!active).catch(() => {})
          await win.setFocusable(active).catch(() => {})
          await win.show().catch(() => {})
          // Pull focus when a prompt opens so the 1-9 / Esc keys work at once.
          if (active) await win.setFocus().catch(() => {})
        }
      } finally {
        running.current = false
      }
    }

    void apply()
    return () => {
      cancelled = true
    }
  }, [enabled, promptActive])
}
