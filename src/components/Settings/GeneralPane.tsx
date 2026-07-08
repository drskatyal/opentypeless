import { useState, useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, MessageCircle } from 'lucide-react'
import { isMacPlatform, useAppStore } from '../../stores/appStore'
import type { AppConfig, HotkeyMode, OutputMode } from '../../stores/appStore'
import {
  pauseHotkey,
  resumeHotkey,
  checkAccessibilityPermission,
  requestAccessibilityPermission,
  waitForAccessibilityPermission,
  getPlatformCapabilities,
  getHotkeyStatus,
  startAskFlow,
} from '../../lib/tauri'
import type { HotkeyStatus } from '../../lib/tauri'
import { SegmentedControl } from './shared/SegmentedControl'
import { Toggle } from './shared/Toggle'

// Keys that can be used as hotkeys without a modifier
const STANDALONE_KEYS = new Set([
  'Space',
  'Tab',
  'Enter',
  'Backspace',
  'Escape',
  'Delete',
  'Insert',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'Up',
  'Down',
  'Left',
  'Right',
  'F1',
  'F2',
  'F3',
  'F4',
  'F5',
  'F6',
  'F7',
  'F8',
  'F9',
  'F10',
  'F11',
  'F12',
])

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

interface HotkeyRecorderProps {
  value: string
  onSaved: (hotkey: string) => void
  validateHotkey?: (hotkey: string) => string | null
  specialOptions?: Array<{ value: string; label: string }>
}

function HotkeyRecorder({ value, onSaved, validateHotkey, specialOptions }: HotkeyRecorderProps) {
  const { t } = useTranslation()
  const isMac = isMacPlatform()
  const [recording, setRecording] = useState(false)
  const [pending, setPending] = useState<string | null>(null)
  const [modifierHint, setModifierHint] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const autoConfirmTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const confirmHotkey = useCallback(
    (hotkey: string) => {
      if (autoConfirmTimer.current) {
        clearTimeout(autoConfirmTimer.current)
        autoConfirmTimer.current = null
      }
      setRecording(false)
      setModifierHint(null)
      setPending(null)
      const validationError = validateHotkey?.(hotkey)
      if (validationError) {
        setError(validationError)
        resumeHotkey().catch((e) => setError(String(e)))
        return
      }
      setError(null)
      onSaved(hotkey)
      resumeHotkey().catch((e) => setError(String(e)))
    },
    [onSaved, validateHotkey],
  )

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()

      // Build modifier prefix
      const parts: string[] = []
      if (isMac && e.metaKey) parts.push('Command')
      if (e.ctrlKey) parts.push('Ctrl')
      if (e.altKey) parts.push(isMac ? 'Option' : 'Alt')
      if (e.shiftKey) parts.push('Shift')
      if (!isMac && e.metaKey) parts.push('Meta')

      // If only modifier keys are pressed, show hint like "Alt+..."
      if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
        setModifierHint(parts.length > 0 ? parts.join('+') + '+...' : null)
        return
      }

      setModifierHint(null)

      const keyMap: Record<string, string> = {
        ' ': 'Space',
        Tab: 'Tab',
        Enter: 'Enter',
        Backspace: 'Backspace',
        Escape: 'Escape',
        Delete: 'Delete',
        Insert: 'Insert',
        Home: 'Home',
        End: 'End',
        PageUp: 'PageUp',
        PageDown: 'PageDown',
        ArrowUp: 'Up',
        ArrowDown: 'Down',
        ArrowLeft: 'Left',
        ArrowRight: 'Right',
        '。': '.',
        '?': '/',
      }

      let keyName = keyMap[e.key] || e.key
      if (keyName.length === 1) keyName = keyName.toUpperCase()

      // Letters and digits require at least one modifier to avoid interfering with typing
      if (parts.length === 0 && !STANDALONE_KEYS.has(keyName)) return

      parts.push(keyName)
      const combo = parts.join('+')
      setPending(combo)

      // Auto-confirm after 1.5 seconds
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
      autoConfirmTimer.current = setTimeout(() => {
        confirmHotkey(combo)
      }, 1500)
    },
    [confirmHotkey, isMac],
  )

  const handleKeyUp = useCallback(() => {
    setModifierHint(null)
  }, [])

  useEffect(() => {
    if (!recording) return
    window.addEventListener('keydown', handleKeyDown, true)
    window.addEventListener('keyup', handleKeyUp, true)
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true)
      window.removeEventListener('keyup', handleKeyUp, true)
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
    }
  }, [recording, handleKeyDown, handleKeyUp])

  const handleClick = () => {
    if (recording && pending) {
      // Confirm immediately on click
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
      confirmHotkey(pending)
    } else if (recording) {
      // Cancel recording — re-register the old hotkey
      setRecording(false)
      setPending(null)
      setModifierHint(null)
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
      resumeHotkey().catch(() => {})
    } else {
      // Start recording — unregister global shortcut so webview can capture keys
      pauseHotkey().catch(() => {})
      setRecording(true)
      setPending(null)
      setError(null)
    }
  }

  return (
    <div>
      <button
        onClick={handleClick}
        className={`w-full px-3 py-2.5 rounded-[10px] text-[13px] font-mono text-left border transition-colors cursor-pointer ${
          recording
            ? 'bg-bg-tertiary border-text-secondary text-text-primary ring-2 ring-text-secondary/20'
            : 'bg-bg-secondary border-transparent text-text-primary hover:border-border'
        }`}
      >
        {recording ? pending || modifierHint || t('settings.pressKeyCombination') : value}
      </button>
      {recording && pending && (
        <p className="text-[11px] text-text-tertiary mt-1.5">{t('settings.clickToConfirm')}</p>
      )}
      {recording && specialOptions && specialOptions.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {specialOptions.map((option) => (
            <button
              key={option.value}
              type="button"
              onClick={() => confirmHotkey(option.value)}
              className="rounded-full border border-border bg-bg-secondary px-2.5 py-1 text-[11px] text-text-secondary transition-colors hover:border-border-focus hover:text-text-primary"
            >
              {option.label}
            </button>
          ))}
        </div>
      )}
      {error && <p className="text-[11px] text-error mt-1.5">{error}</p>}
    </div>
  )
}

export function GeneralPane() {
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const platformCapabilities = useAppStore((s) => s.platformCapabilities)
  const setPlatformCapabilities = useAppStore((s) => s.setPlatformCapabilities)
  const hotkeyRegistrationError = useAppStore((s) => s.hotkeyRegistrationError)
  const { t } = useTranslation()
  const isMac = isMacPlatform()
  const macAccessibilityNeeded = isMac && needsMacAccessibility(config)
  const [a11yTrusted, setA11yTrusted] = useState<boolean | null>(null)
  const [hotkeyStatus, setHotkeyStatus] = useState<HotkeyStatus | null>(null)
  const [advancedOpen, setAdvancedOpen] = useState(false)

  useEffect(() => {
    if (platformCapabilities) return
    getPlatformCapabilities()
      .then(setPlatformCapabilities)
      .catch((err) => {
        console.error('Failed to load platform capabilities:', err)
      })
  }, [platformCapabilities, setPlatformCapabilities])

  useEffect(() => {
    if (macAccessibilityNeeded) {
      checkAccessibilityPermission().then(setA11yTrusted)
      const onFocus = () => checkAccessibilityPermission().then(setA11yTrusted)
      window.addEventListener('focus', onFocus)
      return () => window.removeEventListener('focus', onFocus)
    }
  }, [macAccessibilityNeeded])

  useEffect(() => {
    let cancelled = false
    getHotkeyStatus()
      .then((status) => {
        if (!cancelled) setHotkeyStatus(status)
      })
      .catch((err) => {
        console.error('Failed to load hotkey status:', err)
      })
    return () => {
      cancelled = true
    }
  }, [config.hotkey, config.ask_hotkey, hotkeyRegistrationError])

  const handleGrantPermission = useCallback(async () => {
    await requestAccessibilityPermission()
    const trusted = await waitForAccessibilityPermission()
    setA11yTrusted(trusted)
  }, [])

  const handleOpenAsk = useCallback(() => {
    startAskFlow().catch((err) => {
      console.error('Failed to start Ask flow:', err)
    })
  }, [])

  const hotkeyStatusMessage = hotkeyStatus?.conflict
    ? t('settings.hotkeyConflict')
    : hotkeyStatus && (!hotkeyStatus.dictation.valid || !hotkeyStatus.ask.valid)
      ? t('settings.hotkeyInvalid')
      : null
  const dictationSpecialOptions = isMac
    ? [{ value: 'Fn', label: 'Fn' }]
    : platformCapabilities?.os === 'windows'
      ? [{ value: 'RightAlt', label: 'Right Alt' }]
      : []
  const askSpecialOptions = isMac
    ? [{ value: 'Fn+Space', label: 'Fn + Space' }]
    : platformCapabilities?.os === 'windows'
      ? [{ value: 'RightAlt+Space', label: 'Right Alt + Space' }]
      : []

  return (
    <div className="space-y-6">
      <Section title={t('settings.hotkey')}>
        <div className="space-y-3">
          <div>
            <p className="mb-1.5 text-[12px] font-medium text-text-secondary">
              {t('settings.dictationHotkey')}
            </p>
            <HotkeyRecorder
              value={config.hotkey}
              onSaved={(hotkey) => updateConfig({ hotkey })}
              specialOptions={dictationSpecialOptions}
              validateHotkey={(hotkey) =>
                config.ask_hotkey && hotkey === config.ask_hotkey
                  ? t('settings.hotkeyConflict')
                  : null
              }
            />
          </div>
          <div>
            <p className="mb-1.5 text-[12px] font-medium text-text-secondary">
              {t('settings.askHotkey')}
            </p>
            <div className="flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <HotkeyRecorder
                  value={config.ask_hotkey || '—'}
                  onSaved={(ask_hotkey) => updateConfig({ ask_hotkey })}
                  specialOptions={askSpecialOptions}
                  validateHotkey={(ask_hotkey) =>
                    ask_hotkey === config.hotkey ? t('settings.hotkeyConflict') : null
                  }
                />
              </div>
              <button
                type="button"
                aria-label={t('settings.tryAsk')}
                title={t('settings.tryAsk')}
                onClick={handleOpenAsk}
                className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[8px] border border-transparent bg-bg-secondary text-text-tertiary transition-colors hover:border-border hover:text-text-primary"
              >
                <MessageCircle size={14} />
              </button>
            </div>
          </div>
        </div>
        {!platformCapabilities?.globalHotkeyReliable && (
          <p className="mt-2 rounded-[8px] border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[12px] leading-relaxed text-text-secondary">
            {t('settings.waylandHotkeyLimited')}
          </p>
        )}
        {hotkeyRegistrationError && (
          <p className="mt-2 rounded-[8px] border border-error/30 bg-error/10 px-3 py-2 text-[12px] leading-relaxed text-error">
            {t('settings.hotkeyRegistrationFailed')}
          </p>
        )}
        {hotkeyStatusMessage && (
          <p className="mt-2 rounded-[8px] border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[12px] leading-relaxed text-text-secondary">
            {hotkeyStatusMessage}
          </p>
        )}
        <div className="mt-3">
          <SegmentedControl
            options={[
              { value: 'hold', label: t('settings.holdToTalk') },
              { value: 'toggle', label: t('settings.toggleOnOff') },
            ]}
            value={config.hotkey_mode}
            onChange={(v) => updateConfig({ hotkey_mode: v as HotkeyMode })}
          />
        </div>
      </Section>

      <Section title={t('settings.outputMode')}>
        <SegmentedControl
          options={[
            { value: 'keyboard', label: t('settings.keyboardSimulation') },
            { value: 'clipboard', label: t('settings.clipboardPaste') },
          ]}
          value={config.output_mode}
          onChange={(v) => {
            const outputMode = v as OutputMode
            updateConfig({
              output_mode: outputMode,
              insertion_strategy: outputMode === 'clipboard' ? 'clipboardPaste' : 'auto',
            })
          }}
        />
        {config.output_mode === 'clipboard' &&
          platformCapabilities &&
          !platformCapabilities.clipboardAutoPasteReliable && (
            <p className="mt-2 rounded-[8px] border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[12px] leading-relaxed text-text-secondary">
              {t('settings.waylandClipboardCopyOnly')}
            </p>
          )}
      </Section>

      {macAccessibilityNeeded && a11yTrusted === false && (
        <Section title={t('settings.accessibilityPermission')}>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <span className="h-2 w-2 rounded-full bg-amber-500" />
              <span className="text-[13px] text-text-primary">
                {t('settings.accessibilityRequired')}
              </span>
            </div>
            <button
              type="button"
              onClick={handleGrantPermission}
              className="rounded-full border-none bg-accent px-3 py-1.5 text-[12px] font-medium text-white transition-colors hover:bg-accent-hover"
            >
              {t('settings.grantPermission')}
            </button>
          </div>
        </Section>
      )}

      <div>
        <button
          type="button"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
          className="flex w-full items-center justify-between rounded-[10px] border border-border bg-bg-secondary/40 px-3 py-2 text-[13px] font-medium text-text-primary transition-colors hover:border-border-focus"
        >
          <span>{t('settings.advancedGeneral')}</span>
          <ChevronDown
            size={14}
            className={`text-text-tertiary transition-transform ${advancedOpen ? 'rotate-180' : ''}`}
          />
        </button>

        {advancedOpen && (
          <div className="mt-4 space-y-3">
            <Toggle
              checked={config.auto_start}
              onChange={(checked) => updateConfig({ auto_start: checked })}
              label={t('settings.launchAtStartup')}
            />
            <Toggle
              checked={config.history_enabled}
              onChange={(checked) => updateConfig({ history_enabled: checked })}
              label={t('settings.saveHistory')}
            />
            <Toggle
              checked={config.capsule_auto_hide}
              onChange={(checked) => updateConfig({ capsule_auto_hide: checked })}
              label={t('settings.hideCapsuleWhenIdle')}
            />
          </div>
        )}
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="text-[11px] font-medium text-text-tertiary uppercase tracking-wider mb-2.5">
        {title}
      </h3>
      {children}
    </div>
  )
}
