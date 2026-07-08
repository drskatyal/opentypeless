import { create } from 'zustand'

export type PipelineState =
  | 'idle'
  | 'preparing'
  | 'recording'
  | 'transcribing'
  | 'polishing'
  | 'outputting'
  | 'ask_recording'
  | 'ask_thinking'

export type SttProvider =
  | 'deepgram'
  | 'assemblyai'
  | 'volcengine-doubao'
  | 'glm-asr'
  | 'openai-whisper'
  | 'groq-whisper'
  | 'siliconflow'
  | 'apple-speech'
  | 'custom-whisper'
  | 'cloud'
export type LlmProvider =
  | 'zhipu'
  | 'deepseek'
  | 'siliconflow'
  | 'openai'
  | 'gemini'
  | 'moonshot'
  | 'doubao'
  | 'qwen'
  | 'groq'
  | 'claude'
  | 'ollama'
  | 'openrouter'
  | 'cloud'
export type OutputMode = 'keyboard' | 'clipboard'
export type PasteShortcut = 'ctrlV' | 'ctrlShiftV' | 'shiftInsert'
export type WindowsSendInputNewlineMode = 'enter' | 'shiftEnter' | 'crlf'
export type InsertionStrategy =
  | 'auto'
  | 'keyboard'
  | 'clipboardPaste'
  | 'clipboardCopyOnly'
  | 'windowsSendInput'
export type InsertStatus = 'inserted' | 'copiedFallback' | 'failed' | 'partiallyInserted'
export type HotkeyMode = 'hold' | 'toggle'
export type Theme = 'light' | 'dark' | 'system'
export type PolishChineseScript = 'preserve' | 'simplified' | 'traditional'
export type PolishStyle = 'minimal' | 'clean' | 'structured' | 'professional'
export type SceneSource = 'custom' | 'builtin' | 'cloud'

export interface ShortcutBinding {
  primary: string
  modifiers: string[]
}

export interface HotkeyConfig {
  dictation: ShortcutBinding
  ask: ShortcutBinding | null
  translate: ShortcutBinding | null
  editSelection: ShortcutBinding | null
  switchScene: ShortcutBinding | null
  openApp: ShortcutBinding | null
  dictationMode: HotkeyMode
}

export interface PlatformCapabilities {
  os: 'macos' | 'windows' | 'linux' | 'unknown'
  sessionType: 'wayland' | 'x11' | 'unknown'
  globalHotkeyReliable: boolean
  keyboardOutputReliable: boolean
  clipboardAutoPasteReliable: boolean
}

export interface HistoryEntry {
  id: number
  created_at: string
  app_name: string
  app_type: string
  raw_text: string
  polished_text: string
  language: string | null
  duration_ms: number | null
  active_scene_id: string | null
  active_scene_source: SceneSource | string | null
  active_scene_name: string | null
  active_scene_prompt_chars: number | null
  active_scene_prompt_truncated: boolean
  output_status: string | null
  output_error: string | null
}

export interface InsertResult {
  status: InsertStatus
  strategyUsed: InsertionStrategy
  charsInserted: number
  charsCopied: number
  warningCode: string | null
  message: string | null
}

export interface DictionaryEntry {
  id: number
  word: string
  pronunciation: string | null
}

export interface CorrectionRule {
  id: number
  pattern: string
  replacement: string
  enabled: boolean
}

export interface CustomScene {
  id: string
  name: string
  description: string
  prompt_template: string
  created_at: string
  updated_at: string
}

export interface ActiveScene {
  id: string
  source: SceneSource
  name: string
  prompt_template: string
}

export interface AppConfig {
  stt_provider: SttProvider
  stt_api_key: string
  stt_custom_api_key: string
  stt_custom_preset: 'speaches' | 'custom'
  stt_custom_base_url: string
  stt_custom_model: string
  stt_volcengine_resource_id: string
  stt_language: string
  llm_provider: LlmProvider
  llm_api_key: string
  llm_model: string
  llm_base_url: string
  polish_enabled: boolean
  polish_style: PolishStyle
  polish_custom_prompt: string
  polish_chinese_script: PolishChineseScript
  custom_scenes: CustomScene[]
  active_scene: ActiveScene | null
  translate_enabled: boolean
  target_lang: string
  hotkey: string
  ask_hotkey: string
  hotkey_mode: HotkeyMode
  hotkeys: HotkeyConfig
  output_mode: OutputMode
  insertion_strategy: InsertionStrategy
  restore_clipboard_after_paste: boolean
  paste_shortcut: PasteShortcut
  windows_sendinput_newline_mode: WindowsSendInputNewlineMode
  streaming_insert_enabled: boolean
  selected_text_enabled: boolean
  theme: Theme
  auto_start: boolean
  close_to_tray: boolean
  start_minimized: boolean
  max_recording_seconds: number
  history_enabled: boolean
  history_retention_days: number
  history_max_entries: number
  ui_language: string
  capsule_auto_hide: boolean
}

export type TestStatus = 'idle' | 'testing' | 'success' | 'error'

interface AppState {
  // Pipeline
  pipelineState: PipelineState
  setPipelineState: (state: PipelineState) => void

  // Recording
  audioVolume: number
  setAudioVolume: (v: number) => void
  partialTranscript: string
  setPartialTranscript: (t: string) => void
  finalTranscript: string
  setFinalTranscript: (t: string) => void
  polishedText: string
  setPolishedText: (t: string) => void
  appendPolishedChunk: (chunk: string) => void
  recordingDuration: number
  setRecordingDuration: (d: number) => void
  targetApp: string
  setTargetApp: (app: string) => void
  lastInsertResult: InsertResult | null
  setLastInsertResult: (result: InsertResult | null) => void

  // Config
  config: AppConfig
  setConfig: (config: AppConfig) => void
  updateConfig: (partial: Partial<AppConfig>) => void
  applyPersistedConfigPatch: (patch: Partial<AppConfig>) => void

  // History
  history: HistoryEntry[]
  setHistory: (h: HistoryEntry[]) => void

  // Dictionary
  dictionary: DictionaryEntry[]
  setDictionary: (d: DictionaryEntry[]) => void
  correctionRules: CorrectionRule[]
  setCorrectionRules: (rules: CorrectionRule[]) => void

  // Onboarding
  onboardingCompleted: boolean
  setOnboardingCompleted: (done: boolean) => void
  onboardingStep: number
  setOnboardingStep: (step: number) => void
  onboardingMode: 'cloud' | 'byok' | null
  setOnboardingMode: (mode: 'cloud' | 'byok' | null) => void

  // Capsule
  capsuleExpanded: boolean
  setCapsuleExpanded: (expanded: boolean) => void

  // Connection test status
  sttTestStatus: TestStatus
  setSttTestStatus: (s: TestStatus) => void
  llmTestStatus: TestStatus
  setLlmTestStatus: (s: TestStatus) => void

  // Latency benchmark results (ms), null = not yet measured
  sttLatencyMs: number | null
  setSttLatencyMs: (ms: number | null) => void
  llmLatencyMs: number | null
  setLlmLatencyMs: (ms: number | null) => void

  // LLM model list cache (persists across tab switches)
  llmModels: string[]
  setLlmModels: (models: string[]) => void

  // Pipeline error
  pipelineError: string | null
  setPipelineError: (error: string | null) => void

  // macOS Accessibility permission
  accessibilityTrusted: boolean
  setAccessibilityTrusted: (trusted: boolean) => void
  platformCapabilities: PlatformCapabilities | null
  setPlatformCapabilities: (capabilities: PlatformCapabilities | null) => void
  hotkeyRegistrationError: string | null
  setHotkeyRegistrationError: (error: string | null) => void

  // Context menu
  contextMenuOpen: boolean
  setContextMenuOpen: (open: boolean) => void
  contextMenuReady: boolean
  setContextMenuReady: (ready: boolean) => void

  // Reset recording state
  resetRecording: () => void

  // Config snapshot for dirty detection
  savedConfig: AppConfig | null
  setSavedConfig: (config: AppConfig) => void
  resetConfig: () => void
}

export const isMacPlatform = () =>
  typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0

export const isWindowsPlatform = () =>
  typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('WIN') >= 0

function defaultDictationHotkey(): string {
  if (isMacPlatform()) return 'Fn'
  if (isWindowsPlatform()) return 'RightAlt'
  return 'Ctrl+/'
}

function defaultDictationHotkeyMode(): HotkeyMode {
  return isMacPlatform() || isWindowsPlatform() ? 'toggle' : 'hold'
}

function defaultAskHotkey(): string {
  if (isMacPlatform()) return 'Fn+Space'
  if (isWindowsPlatform()) return 'RightAlt+Space'
  return 'Ctrl+.'
}

function defaultTranslateHotkey(): string | null {
  if (isMacPlatform()) return 'Fn+LeftShift'
  if (isWindowsPlatform()) return 'RightAlt+LeftShift'
  return null
}

const modifierOrder = ['Fn', 'RightAlt', 'Command', 'Super', 'Ctrl', 'Option', 'Alt', 'Shift']

function normalizeModifier(value: string): string | null {
  switch (value.trim().toLowerCase()) {
    case 'fn':
    case 'function':
      return 'Fn'
    case 'rightalt':
    case 'right_alt':
    case 'right-alt':
    case 'altright':
    case 'alt_right':
    case 'alt-right':
      return 'RightAlt'
    case 'cmd':
    case 'command':
      return 'Command'
    case 'meta':
    case 'super':
    case 'win':
      return 'Super'
    case 'ctrl':
    case 'control':
      return 'Ctrl'
    case 'option':
      return 'Option'
    case 'alt':
      return 'Alt'
    case 'shift':
      return 'Shift'
    default:
      return null
  }
}

function normalizePrimary(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return null

  const lower = trimmed.toLowerCase()
  const named: Record<string, string> = {
    space: 'Space',
    tab: 'Tab',
    enter: 'Enter',
    return: 'Enter',
    escape: 'Escape',
    esc: 'Escape',
    leftshift: 'LeftShift',
    left_shift: 'LeftShift',
    'left-shift': 'LeftShift',
    shiftleft: 'LeftShift',
    shift_left: 'LeftShift',
    'shift-left': 'LeftShift',
    slash: '/',
    '/': '/',
    period: '.',
    '.': '.',
    '。': '.',
    comma: ',',
    ',': ',',
    semicolon: ';',
    ';': ';',
    minus: '-',
    '-': '-',
    equal: '=',
    '=': '=',
    bracketleft: '[',
    '[': '[',
    bracketright: ']',
    ']': ']',
  }
  const nativePrimary: Record<string, string> = {
    fn: 'Fn',
    function: 'Fn',
    rightalt: 'RightAlt',
    right_alt: 'RightAlt',
    'right-alt': 'RightAlt',
    altright: 'RightAlt',
    alt_right: 'RightAlt',
    'alt-right': 'RightAlt',
  }
  if (nativePrimary[lower]) return nativePrimary[lower]
  if (named[lower]) return named[lower]
  if (/^f([1-9]|1[0-2])$/.test(lower)) return lower.toUpperCase()
  if (/^[a-z0-9]$/.test(lower)) return lower.toUpperCase()
  return null
}

function bindingFromHotkey(value: string): ShortcutBinding | null {
  const parts = value
    .split('+')
    .map((part) => part.trim())
    .filter(Boolean)
  if (parts.length === 0) return null

  const primary = normalizePrimary(parts[parts.length - 1])
  if (!primary) return null

  const modifiers: string[] = []
  const seen = new Set<string>()
  for (const part of parts.slice(0, -1)) {
    const modifier = normalizeModifier(part)
    if (!modifier) return null
    const semantic = modifier === 'Option' || modifier === 'Alt' ? 'Alt' : modifier
    if (seen.has(semantic)) return null
    seen.add(semantic)
    modifiers.push(modifier)
  }

  modifiers.sort((a, b) => modifierOrder.indexOf(a) - modifierOrder.indexOf(b))
  return { primary, modifiers }
}

function hotkeyFromBinding(binding: ShortcutBinding): string {
  return [...binding.modifiers, binding.primary].join('+')
}

function hotkeyConfigFromLegacy(config: AppConfig): HotkeyConfig {
  return {
    dictation: bindingFromHotkey(config.hotkey) ?? bindingFromHotkey(defaultDictationHotkey())!,
    ask: config.ask_hotkey.trim()
      ? (bindingFromHotkey(config.ask_hotkey) ?? bindingFromHotkey(defaultAskHotkey()))
      : null,
    translate:
      config.hotkeys?.translate ??
      (defaultTranslateHotkey() ? bindingFromHotkey(defaultTranslateHotkey()!) : null),
    editSelection: config.hotkeys?.editSelection ?? null,
    switchScene: config.hotkeys?.switchScene ?? null,
    openApp: config.hotkeys?.openApp ?? null,
    dictationMode:
      config.hotkey_mode === 'toggle'
        ? 'toggle'
        : config.hotkey_mode === 'hold'
          ? 'hold'
          : defaultDictationHotkeyMode(),
  }
}

function syncLegacyHotkeysToTyped(config: AppConfig): AppConfig {
  return { ...config, hotkeys: hotkeyConfigFromLegacy(config) }
}

function syncTypedHotkeysToLegacy(config: AppConfig): AppConfig {
  const hotkeys = config.hotkeys ?? hotkeyConfigFromLegacy(config)
  return {
    ...config,
    hotkey: hotkeyFromBinding(hotkeys.dictation),
    ask_hotkey: hotkeys.ask ? hotkeyFromBinding(hotkeys.ask) : '',
    hotkey_mode: hotkeys.dictationMode,
    hotkeys,
  }
}

function syncHotkeyConfig(previous: AppConfig, partial: Partial<AppConfig>): AppConfig {
  const merged = { ...previous, ...partial }
  if (partial.hotkeys) return syncTypedHotkeysToLegacy(merged)
  if ('hotkey' in partial || 'ask_hotkey' in partial || 'hotkey_mode' in partial) {
    return syncLegacyHotkeysToTyped(merged)
  }
  return merged.hotkeys ? merged : syncLegacyHotkeysToTyped(merged)
}

const defaultConfig: AppConfig = {
  stt_provider: 'glm-asr',
  stt_api_key: '',
  stt_custom_api_key: '',
  stt_custom_preset: 'speaches',
  stt_custom_base_url: 'http://localhost:8000/v1',
  stt_custom_model: 'Systran/faster-whisper-large-v3',
  stt_volcengine_resource_id: 'volc.seedasr.sauc.duration',
  stt_language: 'multi',
  llm_provider: 'openrouter',
  llm_api_key: '',
  llm_model: 'google/gemini-2.5-flash',
  llm_base_url: 'https://openrouter.ai/api/v1',
  polish_enabled: true,
  polish_style: 'clean',
  polish_custom_prompt: '',
  polish_chinese_script: 'preserve',
  custom_scenes: [],
  active_scene: null,
  translate_enabled: false,
  target_lang: 'en',
  hotkey: defaultDictationHotkey(),
  ask_hotkey: defaultAskHotkey(),
  hotkey_mode: defaultDictationHotkeyMode(),
  hotkeys: {
    dictation: bindingFromHotkey(defaultDictationHotkey())!,
    ask: bindingFromHotkey(defaultAskHotkey()),
    translate: defaultTranslateHotkey() ? bindingFromHotkey(defaultTranslateHotkey()!) : null,
    editSelection: null,
    switchScene: null,
    openApp: null,
    dictationMode: defaultDictationHotkeyMode(),
  },
  output_mode: 'keyboard',
  insertion_strategy: 'auto',
  restore_clipboard_after_paste: true,
  paste_shortcut: 'ctrlV',
  windows_sendinput_newline_mode: 'enter',
  streaming_insert_enabled: false,
  selected_text_enabled: false,
  theme: 'system',
  auto_start: true,
  close_to_tray: true,
  start_minimized: false,
  max_recording_seconds: 30,
  history_enabled: true,
  history_retention_days: 0,
  history_max_entries: 5000,
  ui_language: 'en',
  capsule_auto_hide: true,
}

export const useAppStore = create<AppState>((set) => ({
  pipelineState: 'idle',
  setPipelineState: (pipelineState) => set({ pipelineState }),

  audioVolume: 0,
  setAudioVolume: (audioVolume) => set({ audioVolume }),
  partialTranscript: '',
  setPartialTranscript: (partialTranscript) => set({ partialTranscript }),
  finalTranscript: '',
  setFinalTranscript: (finalTranscript) => set({ finalTranscript }),
  polishedText: '',
  setPolishedText: (polishedText) => set({ polishedText }),
  appendPolishedChunk: (chunk) => set((s) => ({ polishedText: s.polishedText + chunk })),
  recordingDuration: 0,
  setRecordingDuration: (recordingDuration) => set({ recordingDuration }),
  targetApp: '',
  setTargetApp: (targetApp) => set({ targetApp }),
  lastInsertResult: null,
  setLastInsertResult: (lastInsertResult) => set({ lastInsertResult }),

  config: defaultConfig,
  setConfig: (config) => set((s) => ({ config: syncHotkeyConfig(s.config, config) })),
  updateConfig: (partial) => set((s) => ({ config: syncHotkeyConfig(s.config, partial) })),
  applyPersistedConfigPatch: (patch) =>
    set((s) => ({
      config: syncHotkeyConfig(s.config, patch),
      savedConfig: s.savedConfig ? syncHotkeyConfig(s.savedConfig, patch) : s.savedConfig,
    })),

  history: [],
  setHistory: (history) => set({ history }),

  dictionary: [],
  setDictionary: (dictionary) => set({ dictionary }),
  correctionRules: [],
  setCorrectionRules: (correctionRules) => set({ correctionRules }),

  onboardingCompleted: false,
  setOnboardingCompleted: (onboardingCompleted) => set({ onboardingCompleted }),
  onboardingStep: 0,
  setOnboardingStep: (onboardingStep) => set({ onboardingStep }),
  onboardingMode: null,
  setOnboardingMode: (onboardingMode) => set({ onboardingMode }),

  capsuleExpanded: false,
  setCapsuleExpanded: (capsuleExpanded) => set({ capsuleExpanded }),

  sttTestStatus: 'idle',
  setSttTestStatus: (sttTestStatus) => set({ sttTestStatus }),
  llmTestStatus: 'idle',
  setLlmTestStatus: (llmTestStatus) => set({ llmTestStatus }),

  sttLatencyMs: null,
  setSttLatencyMs: (sttLatencyMs) => set({ sttLatencyMs }),
  llmLatencyMs: null,
  setLlmLatencyMs: (llmLatencyMs) => set({ llmLatencyMs }),

  llmModels: [],
  setLlmModels: (llmModels) => set({ llmModels }),

  pipelineError: null,
  setPipelineError: (pipelineError) => set({ pipelineError }),

  accessibilityTrusted: true,
  setAccessibilityTrusted: (accessibilityTrusted) => set({ accessibilityTrusted }),
  platformCapabilities: null,
  setPlatformCapabilities: (platformCapabilities) => set({ platformCapabilities }),
  hotkeyRegistrationError: null,
  setHotkeyRegistrationError: (hotkeyRegistrationError) => set({ hotkeyRegistrationError }),

  contextMenuOpen: false,
  setContextMenuOpen: (contextMenuOpen) => set({ contextMenuOpen }),
  contextMenuReady: false,
  setContextMenuReady: (contextMenuReady) => set({ contextMenuReady }),

  resetRecording: () =>
    set({
      audioVolume: 0,
      partialTranscript: '',
      finalTranscript: '',
      polishedText: '',
      recordingDuration: 0,
    }),

  savedConfig: null,
  setSavedConfig: (savedConfig) => set({ savedConfig }),
  resetConfig: () => set((s) => (s.savedConfig ? { config: { ...s.savedConfig } } : {})),
}))
