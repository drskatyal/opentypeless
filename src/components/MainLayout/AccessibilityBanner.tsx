import { useState, useEffect, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { ShieldAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import type { AppConfig } from '../../stores/appStore'
import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
  waitForAccessibilityPermission,
} from '../../lib/tauri'

function isFnDictationHotkey(config: AppConfig) {
  return (
    config.hotkey.trim().toLowerCase() === 'fn' ||
    (config.hotkeys.dictation.primary.trim().toLowerCase() === 'fn' &&
      config.hotkeys.dictation.modifiers.length === 0)
  )
}

function needsMacAccessibility(config: AppConfig) {
  return config.output_mode === 'keyboard' || isFnDictationHotkey(config)
}

export function AccessibilityBanner() {
  const { t } = useTranslation()
  const accessibilityTrusted = useAppStore((s) => s.accessibilityTrusted)
  const setAccessibilityTrusted = useAppStore((s) => s.setAccessibilityTrusted)
  const config = useAppStore((s) => s.config)
  const isMac =
    typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0
  const [dismissed, setDismissed] = useState(false)
  const macAccessibilityNeeded = isMac && needsMacAccessibility(config)

  const show = macAccessibilityNeeded && !accessibilityTrusted && !dismissed

  // Re-show banner when accessibility error fires (user just hit the issue)
  useEffect(() => {
    if (!accessibilityTrusted) setDismissed(false)
  }, [accessibilityTrusted])

  useEffect(() => {
    if (macAccessibilityNeeded && !accessibilityTrusted) {
      const onFocus = () => checkAccessibilityPermission().then(setAccessibilityTrusted)
      window.addEventListener('focus', onFocus)
      return () => window.removeEventListener('focus', onFocus)
    }
  }, [macAccessibilityNeeded, accessibilityTrusted, setAccessibilityTrusted])

  const handleGrant = useCallback(async () => {
    await requestAccessibilityPermission()
    const trusted = await waitForAccessibilityPermission()
    setAccessibilityTrusted(trusted)
  }, [setAccessibilityTrusted])

  return (
    <AnimatePresence>
      {show && (
        <motion.div
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: 'auto', opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: 0.2 }}
          className="overflow-hidden"
        >
          <div className="flex items-center gap-2 px-4 py-2 bg-amber-500/10 border-b border-amber-500/20">
            <ShieldAlert size={14} className="text-amber-500 shrink-0" />
            <span className="text-[12px] text-text-primary flex-1">
              {t('settings.accessibilityRequired')} — {t('settings.accessibilityPermission')}
            </span>
            <button
              onClick={handleGrant}
              className="px-3 py-1 text-[11px] font-medium text-white bg-accent rounded-full border-none cursor-pointer hover:bg-accent-hover transition-colors shrink-0"
            >
              {t('settings.grantPermission')}
            </button>
            <button
              onClick={() => setDismissed(true)}
              className="text-text-tertiary text-[12px] border-none bg-transparent cursor-pointer hover:text-text-secondary shrink-0"
            >
              ✕
            </button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
