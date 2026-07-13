import { useTranslation } from 'react-i18next'
import { MessageCircle } from 'lucide-react'
import { CapsuleWorkIndicator } from './CapsuleWorkIndicator'

export function CapsuleAskThinking() {
  const { t } = useTranslation()

  return (
    <div className="relative z-10 flex h-9 items-center gap-2 px-3">
      <MessageCircle size={13} className="shrink-0 text-white/90" />
      <span className="whitespace-nowrap text-[11px] font-medium text-white">{t('ask.title')}</span>
      <CapsuleWorkIndicator tone="thinking" />
      <p className="min-w-0 flex-1 truncate text-[11px] leading-snug text-white/90">
        {t('ask.thinking')}
      </p>
    </div>
  )
}
