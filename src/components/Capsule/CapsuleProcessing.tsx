import { useTranslation } from 'react-i18next'
import { motion, useReducedMotion } from 'framer-motion'
import { X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'
import { CapsuleWorkIndicator } from './CapsuleWorkIndicator'

export function CapsuleProcessing() {
  const { t } = useTranslation()
  const partialTranscript = useAppStore((s) => s.partialTranscript)
  const reduced = useReducedMotion()

  const displayText = partialTranscript || t('capsule.transcribing')

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort processing:', err)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <motion.div className="relative z-10 flex items-center gap-2 h-9 px-3">
      <CapsuleWorkIndicator tone="steady" />
      <p className="text-[11px] text-white leading-snug truncate flex-1 min-w-0">
        {displayText}
        <motion.span
          className="inline-block w-[2px] h-[11px] bg-white/60 ml-0.5 align-middle"
          animate={reduced ? undefined : { opacity: [1, 0, 1] }}
          transition={{ repeat: Infinity, duration: 0.8 }}
        />
      </p>
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleCancel}
        aria-label={t('capsule.cancelProcessing')}
        className="flex-shrink-0 p-1 rounded-full text-white/70 hover:text-white hover:bg-white/15 transition-colors bg-transparent border-none cursor-pointer"
      >
        <X size={12} />
      </button>
    </motion.div>
  )
}
