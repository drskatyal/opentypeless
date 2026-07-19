import { useTranslation } from 'react-i18next'
import { motion, useReducedMotion } from 'framer-motion'
import { X } from 'lucide-react'
import { abortRecording } from '../../lib/tauri'
import { Waveform } from './Waveform'
import { DurationTimer } from './DurationTimer'
import { TranslateTargetChip } from './TranslateTargetChip'

export function CapsuleRecording() {
  const { t } = useTranslation()
  const reduced = useReducedMotion()

  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation()
    try {
      await abortRecording()
    } catch (err) {
      console.error('Failed to abort recording:', err)
    }
  }

  const stopPointerPropagation = (e: React.PointerEvent) => {
    e.stopPropagation()
  }

  return (
    <motion.div
      className="relative z-10 flex h-9 items-center gap-2.5 pl-3 pr-2.5"
      role="status"
      aria-label={t('capsule.cancelRecording')}
    >
      {/* Pulsing red level indicator */}
      <motion.span
        className="capsule-rec-dot"
        aria-hidden="true"
        animate={reduced ? undefined : { scale: [1, 1.18, 1], opacity: [1, 0.75, 1] }}
        transition={{ repeat: Infinity, duration: 1.4, ease: 'easeInOut' }}
      />
      <Waveform />
      <TranslateTargetChip />
      <div className="flex-1" />
      <DurationTimer />
      <button
        onPointerDown={stopPointerPropagation}
        onPointerUp={stopPointerPropagation}
        onClick={handleCancel}
        aria-label={t('capsule.cancelRecording')}
        className="flex-shrink-0 cursor-pointer rounded-full border-none bg-transparent p-1 text-white/70 transition-colors hover:bg-white/15 hover:text-white"
      >
        <X size={12} />
      </button>
    </motion.div>
  )
}
