import { motion, useReducedMotion } from 'framer-motion'
import { spring } from '../../lib/animations'
import { CapsuleLogo } from './CapsuleLogo'

export function CapsuleIdle() {
  const reduced = useReducedMotion()

  return (
    <motion.div
      className="relative z-10 flex items-center justify-center w-9 h-9 cursor-grab active:cursor-grabbing"
      whileHover={reduced ? undefined : { scale: 1.06 }}
      transition={spring.smooth}
    >
      {/* Subtle breathing wrapper */}
      <motion.div
        animate={
          reduced
            ? undefined
            : {
                scale: [1, 1.02, 1],
              }
        }
        transition={{ repeat: Infinity, duration: 3, ease: 'easeInOut' }}
      >
        <CapsuleLogo size={18} className="drop-shadow-sm" />
      </motion.div>
    </motion.div>
  )
}
