import { motion, useReducedMotion } from 'framer-motion'
import { Mic, Sparkles } from 'lucide-react'
import { spring } from '../../lib/animations'

interface CapsuleIdleProps {
  /** When Act is armed, show the violet/teal "ready to act" variant. */
  armed?: boolean
}

export function CapsuleIdle({ armed = false }: CapsuleIdleProps) {
  const reduced = useReducedMotion()

  return (
    <motion.div
      className="relative z-10 flex h-9 w-9 items-center justify-center cursor-grab active:cursor-grabbing"
      whileHover={reduced ? undefined : { scale: 1.06 }}
      transition={spring.smooth}
      role="img"
      aria-label={armed ? 'Act armed — hold to command' : 'Dictate — hold to record'}
    >
      {/* Breathing inner aura — bleeds toward the glass edge for a living glow */}
      <motion.span
        aria-hidden="true"
        className={`capsule-aura${armed ? ' capsule-aura-violet' : ''}`}
        animate={
          reduced ? { opacity: 0.6 } : { opacity: [0.45, 0.85, 0.45], scale: [0.94, 1.06, 0.94] }
        }
        transition={{ repeat: Infinity, duration: 3.6, ease: 'easeInOut' }}
      />

      {/* Slow rotating conic ring hugging the rim */}
      <span aria-hidden="true" className={`capsule-ring${armed ? ' capsule-ring-violet' : ''}`} />

      {armed ? (
        <Sparkles
          size={15}
          strokeWidth={2.2}
          className="relative z-10 text-[#a78bfa] drop-shadow-sm"
        />
      ) : (
        <Mic size={16} strokeWidth={2.1} className="capsule-mic relative z-10 drop-shadow-sm" />
      )}

      {armed && <span aria-hidden="true" className="capsule-badge-dot" />}
    </motion.div>
  )
}
