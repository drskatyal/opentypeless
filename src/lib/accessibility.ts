import type { AppConfig } from '../stores/appStore'

export function isMacPlatform() {
  return typeof navigator !== 'undefined' && navigator.platform.toUpperCase().includes('MAC')
}

function isFnDictationHotkey(config: AppConfig) {
  const legacyHotkey = config.hotkey?.trim().toLowerCase()
  const dictationBindings = config.hotkeys?.dictationBindings?.length
    ? config.hotkeys.dictationBindings
    : config.hotkeys?.dictation
      ? [config.hotkeys.dictation]
      : []
  return (
    legacyHotkey === 'fn' ||
    dictationBindings.some(
      (binding) => binding.primary?.trim().toLowerCase() === 'fn' && binding.modifiers.length === 0,
    )
  )
}

export function needsMacAccessibility(config: AppConfig) {
  const usesAutomatedInput = config.insertion_strategy !== 'clipboardCopyOnly'
  return isMacPlatform() && (usesAutomatedInput || isFnDictationHotkey(config))
}
