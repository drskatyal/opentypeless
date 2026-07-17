import { useEffect, useRef } from 'react'
import { useReducedMotion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'

const BAR_COUNT = 7
const MIN_HEIGHT = 4
const MAX_HEIGHT = 20

// A gentle centre-weighted envelope so the middle bars swing widest —
// reads as an organic "mouth" rather than a flat equalizer.
const ENVELOPE = Array.from({ length: BAR_COUNT }, (_, i) => {
  const t = i / (BAR_COUNT - 1)
  return 0.55 + 0.45 * Math.sin(t * Math.PI)
})

export function Waveform() {
  const barsRef = useRef<(HTMLDivElement | null)[]>([])
  const rafRef = useRef<number>(0)
  const reduced = useReducedMotion()

  useEffect(() => {
    if (reduced) {
      // Static bars following the envelope when reduced motion is preferred
      barsRef.current.forEach((bar, i) => {
        if (!bar) return
        bar.style.height = `${MIN_HEIGHT + (MAX_HEIGHT - MIN_HEIGHT) * 0.4 * ENVELOPE[i]}px`
        bar.style.opacity = '0.75'
      })
      return
    }

    const animate = () => {
      const volume = useAppStore.getState().audioVolume
      const now = Date.now()
      barsRef.current.forEach((bar, i) => {
        if (!bar) return
        // Multi-sine wobble per bar for an organic, non-repeating motion
        const wobble =
          Math.sin(now / 180 + i * 0.9) * 0.16 +
          Math.sin(now / 90 + i * 1.7) * 0.08 +
          Math.sin(now / 310 - i * 0.5) * 0.06
        const level = (volume * 0.7 + 0.12) * ENVELOPE[i] + wobble
        const normalized = Math.max(0, Math.min(1, level))
        const height = MIN_HEIGHT + (MAX_HEIGHT - MIN_HEIGHT) * normalized
        bar.style.height = `${height}px`
        bar.style.opacity = `${Math.max(0.55, normalized)}`
      })
      rafRef.current = requestAnimationFrame(animate)
    }

    rafRef.current = requestAnimationFrame(animate)
    return () => cancelAnimationFrame(rafRef.current)
  }, [reduced])

  return (
    <div className="capsule-wave" aria-hidden="true">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          ref={(el) => {
            barsRef.current[i] = el
          }}
          className="capsule-wave-bar"
          style={{ height: `${MIN_HEIGHT}px`, opacity: 0.55 }}
        />
      ))}
    </div>
  )
}
