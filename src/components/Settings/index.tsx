import { useState, useEffect, useRef } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { SettingsSidebar, type PaneId } from './SettingsSidebar'
import { GeneralPane } from './GeneralPane'
import { SttPane } from './SttPane'
import { LlmPane } from './LlmPane'
import { ActPane } from './ActPane'
import { DictionaryPane } from './DictionaryPane'
import { ScenesPane } from './ScenesPane'
import { AboutPane } from './AboutPane'
import { DirtyBar } from './shared/DirtyBar'
import { useDirtyConfig } from './shared/useDirtyConfig'

const paneTitleKeys: Record<PaneId, string> = {
  general: 'settings.general',
  stt: 'settings.speechRecognition',
  llm: 'settings.aiPolish',
  act: 'settings.actMode',
  dictionary: 'settings.dictionary',
  scenes: 'settings.scenes',
  about: 'settings.about',
}

const paneDescKeys: Record<PaneId, string> = {
  general: 'settings.generalDesc',
  stt: 'settings.speechRecognitionDesc',
  llm: 'settings.aiPolishDesc',
  act: 'settings.actModeDesc',
  dictionary: 'settings.dictionaryDesc',
  scenes: 'settings.scenesDesc',
  about: 'settings.aboutDesc',
}

export function Settings() {
  const [activePane, setActivePane] = useState<PaneId>('general')
  const contentRef = useRef<HTMLDivElement | null>(null)
  const config = useAppStore((s) => s.config)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)
  const isDirty = useDirtyConfig()
  const { t } = useTranslation()

  // First-run onboarding may enter Settings before MainApp has established backend truth.
  useEffect(() => {
    if (useAppStore.getState().savedConfig === null) setSavedConfig(config)
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    contentRef.current?.scrollTo?.({ top: 0 })
  }, [activePane])

  return (
    <div className="flex h-full w-full min-h-0 bg-bg-primary text-text-primary max-[820px]:flex-col">
      {/* Nav rail */}
      <SettingsSidebar activePane={activePane} onSelect={setActivePane} />

      {/* Content column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Sticky pane header: title + one-line description */}
        <header className="shrink-0 border-b border-border bg-bg-primary/80 px-8 pb-4 pt-5 backdrop-blur max-[820px]:px-5">
          <h1 className="text-[20px] font-semibold tracking-tight text-text-primary max-[520px]:text-[18px]">
            {t(paneTitleKeys[activePane])}
          </h1>
          <p className="mt-0.5 max-w-[62ch] text-[13px] text-text-secondary">
            {t(paneDescKeys[activePane])}
          </p>
        </header>

        {/* Scrollable pane content */}
        <div ref={contentRef} className="flex-1 overflow-y-auto overflow-x-hidden">
          <motion.div
            key={activePane}
            className="mx-auto flex max-w-[720px] flex-col gap-6 px-8 pb-24 pt-6 max-[820px]:px-5"
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.18, ease: 'easeOut' }}
          >
            {activePane === 'general' && <GeneralPane />}
            {activePane === 'stt' && <SttPane />}
            {activePane === 'llm' && <LlmPane />}
            {activePane === 'act' && <ActPane />}
            {activePane === 'dictionary' && <DictionaryPane />}
            {activePane === 'scenes' && <ScenesPane />}
            {activePane === 'about' && <AboutPane />}
          </motion.div>
        </div>

        {/* Sticky save bar */}
        <AnimatePresence>{isDirty && <DirtyBar />}</AnimatePresence>
      </div>
    </div>
  )
}
