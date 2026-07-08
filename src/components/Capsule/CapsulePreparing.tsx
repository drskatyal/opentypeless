import { useTranslation } from 'react-i18next'
import { motion, useReducedMotion } from 'framer-motion'
import { X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'

export function CapsulePreparing() {
  const { t } = useTranslation()
  const reduced = useReducedMotion()

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort preparing recording:', err)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-2 h-9 px-3">
      <motion.span
        className="w-2 h-2 rounded-full bg-white/75 flex-shrink-0"
        animate={reduced ? undefined : { opacity: [0.45, 1, 0.45], scale: [0.92, 1, 0.92] }}
        transition={{ repeat: Infinity, duration: 1.2, ease: 'easeInOut' }}
      />
      <p className="text-[11px] text-white/90 leading-snug truncate flex-1 min-w-0">
        {t('capsule.preparing')}
      </p>
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleCancel}
        aria-label={t('capsule.cancelRecording')}
        className="flex-shrink-0 p-1 rounded-full text-white/70 hover:text-white hover:bg-white/15 transition-colors bg-transparent border-none cursor-pointer"
      >
        <X size={12} />
      </button>
    </motion.div>
  )
}
