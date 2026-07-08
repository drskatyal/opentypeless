import { motion, useReducedMotion } from 'framer-motion'

interface CapsuleWorkIndicatorProps {
  tone?: 'steady' | 'thinking'
}

export function CapsuleWorkIndicator({ tone = 'steady' }: CapsuleWorkIndicatorProps) {
  const reduced = useReducedMotion()
  const baseDelay = tone === 'thinking' ? 0.16 : 0.12

  return (
    <div className="flex items-center gap-[3px] flex-shrink-0" aria-hidden="true">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="block w-[4px] h-[4px] rounded-full bg-white/80"
          animate={reduced ? undefined : { opacity: [0.35, 1, 0.35], scale: [0.86, 1.08, 0.86] }}
          transition={{
            repeat: Infinity,
            duration: tone === 'thinking' ? 1.05 : 0.95,
            delay: i * baseDelay,
            ease: 'easeInOut',
          }}
        />
      ))}
    </div>
  )
}
