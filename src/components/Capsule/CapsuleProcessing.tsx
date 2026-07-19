import { useTranslation } from 'react-i18next'
import { motion } from 'framer-motion'
import { X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { useAppStore } from '../../stores/appStore'
import { CapsuleWorkIndicator } from './CapsuleWorkIndicator'

export function CapsuleProcessing() {
  const { t } = useTranslation()
  const partialTranscript = useAppStore((s) => s.partialTranscript)

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
    <motion.div
      className="relative z-10 flex h-9 items-center gap-2 px-3"
      role="status"
      aria-label={t('capsule.transcribing')}
    >
      <span className="capsule-spinner" aria-hidden="true" />
      <p className="min-w-0 flex-1 truncate text-[11px] font-medium leading-snug text-white">
        {displayText}
      </p>
      <CapsuleWorkIndicator tone="steady" />
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleCancel}
        aria-label={t('capsule.cancelProcessing')}
        className="flex-shrink-0 cursor-pointer rounded-full border-none bg-transparent p-1 text-white/70 transition-colors hover:bg-white/15 hover:text-white"
      >
        <X size={12} />
      </button>
    </motion.div>
  )
}
