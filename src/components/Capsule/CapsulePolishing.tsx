import { useTranslation } from 'react-i18next'
import { motion } from 'framer-motion'
import { Sparkles, X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'

export function CapsulePolishing() {
  const { t } = useTranslation()

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort polishing:', err)
    }
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-2 h-9 px-3">
      <motion.div
        className="flex-shrink-0"
        animate={{ scale: [1, 1.12, 1], rotate: [0, 8, -8, 0] }}
        transition={{ duration: 1.8, repeat: Infinity, ease: 'easeInOut' }}
      >
        <Sparkles size={14} className="text-accent" strokeWidth={2.3} />
      </motion.div>
      <p className="text-[11px] text-white leading-snug truncate flex-1 min-w-0">
        {t('capsule.thinking')}
      </p>
      {/* shimmer sweep — the "AI polish" personality */}
      <div className="relative h-1 w-9 flex-shrink-0 overflow-hidden rounded-full bg-white/15">
        <motion.div
          className="absolute inset-y-0 w-1/2 rounded-full bg-gradient-to-r from-transparent via-accent to-transparent"
          animate={{ x: ['-110%', '210%'] }}
          transition={{ duration: 1.1, repeat: Infinity, ease: 'easeInOut' }}
        />
      </div>
      <button
        onClick={handleCancel}
        aria-label={t('capsule.cancelPolishing')}
        className="flex-shrink-0 p-1 rounded-full text-white/70 hover:text-white hover:bg-white/15 transition-colors bg-transparent border-none cursor-pointer"
      >
        <X size={12} />
      </button>
    </motion.div>
  )
}
