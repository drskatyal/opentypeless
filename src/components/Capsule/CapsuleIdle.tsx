import { motion, useReducedMotion } from 'framer-motion'
import { Mic } from 'lucide-react'
import { spring } from '../../lib/animations'

// Aurora accent used for the breathing glow (dark theme accent).
// The pill body stays black in both themes; the glow reads as aurora.

export function CapsuleIdle() {
  const reduced = useReducedMotion()

  return (
    <motion.div
      className="relative z-10 flex items-center justify-center w-9 h-9 cursor-grab active:cursor-grabbing"
      whileHover={reduced ? undefined : { scale: 1.06 }}
      transition={spring.smooth}
    >
      {/* Black mic pill — a physical-object metaphor that stays black in both themes */}
      <motion.div
        aria-label="Dictate"
        role="img"
        className="relative flex items-center justify-center w-7 h-7 rounded-full bg-[#0B0E14] ring-1 ring-white/10"
        animate={
          reduced
            ? undefined
            : {
                scale: [1, 1.04, 1],
                boxShadow: [
                  `0 0 0 0 rgba(111, 231, 203, 0), 0 1px 3px rgba(0, 0, 0, 0.4)`,
                  `0 0 10px 2px rgba(111, 231, 203, 0.35), 0 1px 3px rgba(0, 0, 0, 0.4)`,
                  `0 0 0 0 rgba(111, 231, 203, 0), 0 1px 3px rgba(0, 0, 0, 0.4)`,
                ],
              }
        }
        whileHover={
          reduced
            ? undefined
            : {
                boxShadow: `0 0 14px 3px rgba(111, 231, 203, 0.55), 0 1px 3px rgba(0, 0, 0, 0.4)`,
              }
        }
        transition={{ repeat: Infinity, duration: 3, ease: 'easeInOut' }}
        style={{
          boxShadow: reduced ? '0 1px 3px rgba(0, 0, 0, 0.4)' : undefined,
        }}
      >
        <Mic size={16} strokeWidth={2} className="text-white/90 drop-shadow-sm" />
      </motion.div>
    </motion.div>
  )
}
