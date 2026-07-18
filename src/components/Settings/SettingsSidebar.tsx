import { Settings, Mic, Sparkles, BookOpen, Info, LayoutGrid, Wand2, AudioLines } from 'lucide-react'
import { motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { spring } from '../../lib/animations'
import { APP_VERSION } from '../../lib/constants'

const PANES = [
  { id: 'general', labelKey: 'settings.general', icon: Settings },
  { id: 'stt', labelKey: 'settings.speechRecognition', icon: Mic },
  { id: 'llm', labelKey: 'settings.aiPolish', icon: Sparkles },
  { id: 'act', labelKey: 'settings.actMode', icon: Wand2 },
  { id: 'dictionary', labelKey: 'settings.dictionary', icon: BookOpen },
  { id: 'scenes', labelKey: 'settings.scenes', icon: LayoutGrid },
  { id: 'about', labelKey: 'settings.about', icon: Info },
] as const

export type PaneId = (typeof PANES)[number]['id']

interface Props {
  activePane: PaneId
  onSelect: (id: PaneId) => void
}

/**
 * Claude-Desktop-style nav rail. Desktop: a vertical rail with a brand header and
 * a footer version chip. ≤820px: collapses to a horizontal, scrollable top strip.
 * ≤520px: labels drop to an icon-only rail.
 */
export function SettingsSidebar({ activePane, onSelect }: Props) {
  const { t } = useTranslation()

  return (
    <nav
      aria-label={t('settings.title')}
      className="flex w-[220px] shrink-0 flex-col border-r border-border bg-bg-secondary max-[820px]:w-full max-[820px]:flex-row max-[820px]:items-center max-[820px]:overflow-x-auto max-[820px]:border-b max-[820px]:border-r-0"
    >
      {/* Brand */}
      <div className="flex items-center gap-2.5 px-4 pb-3.5 pt-[18px] max-[820px]:shrink-0 max-[820px]:px-3.5 max-[820px]:py-2.5">
        <span
          aria-hidden="true"
          className="grid h-[26px] w-[26px] shrink-0 place-items-center rounded-lg bg-accent text-white"
        >
          <AudioLines size={15} />
        </span>
        <div className="max-[520px]:hidden">
          <div className="text-[14px] font-semibold tracking-tight text-text-primary">
            OpenTypeless
          </div>
          <div className="text-[11.5px] text-text-tertiary max-[820px]:hidden">
            {t('settings.title')}
          </div>
        </div>
      </div>

      {/* Nav items */}
      <div
        role="tablist"
        className="flex flex-col gap-0.5 overflow-y-auto px-2.5 py-1.5 max-[820px]:flex-1 max-[820px]:flex-row max-[820px]:gap-1 max-[820px]:overflow-x-auto max-[820px]:py-2"
      >
        {PANES.map((pane) => {
          const Icon = pane.icon
          const isActive = activePane === pane.id
          return (
            <motion.button
              key={pane.id}
              role="tab"
              aria-selected={isActive}
              aria-current={isActive}
              onClick={() => onSelect(pane.id)}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scaleX: 1.04, scaleY: 0.96 }}
              transition={spring.jellyGentle}
              className={`relative flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none px-2.5 py-2 text-left text-[13.5px] transition-colors max-[820px]:w-auto max-[820px]:whitespace-nowrap ${
                isActive
                  ? 'bg-accent-light font-medium text-accent-text'
                  : 'bg-transparent text-text-secondary hover:bg-bg-tertiary/50 hover:text-text-primary'
              }`}
            >
              <Icon size={17} className="shrink-0" />
              <span className="max-[520px]:hidden">{t(pane.labelKey)}</span>
            </motion.button>
          )
        })}
      </div>

      {/* Footer */}
      <div className="mt-auto border-t border-border px-4 py-2.5 max-[820px]:hidden">
        <span className="text-[11.5px] tabular-nums text-text-tertiary">{APP_VERSION}</span>
      </div>
    </nav>
  )
}
