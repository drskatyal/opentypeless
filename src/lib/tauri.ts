import { invoke } from '@tauri-apps/api/core'
import type {
  AppConfig,
  HistoryEntry,
  DictionaryEntry,
  CorrectionRule,
  PlatformCapabilities,
} from '../stores/appStore'

// Pipeline commands
export async function startRecording(): Promise<void> {
  return invoke('start_recording')
}

export async function stopRecording(): Promise<void> {
  return invoke('stop_recording')
}

export async function abortRecording(): Promise<void> {
  return invoke('abort_recording')
}

// Config commands
export async function getConfig(): Promise<AppConfig> {
  return invoke('get_config')
}

export async function updateConfig(config: AppConfig): Promise<void> {
  return invoke('update_config', { config })
}

export interface CredentialStatus {
  namespace: string
  provider: string
  hasSecret: boolean
  updatedAt: string | null
  storage: 'unavailable' | 'os-vault' | 'session-only' | 'legacy-warning'
}

export async function getCredentialStatus(
  namespace: 'stt' | 'llm',
  provider: string,
): Promise<CredentialStatus> {
  return invoke('get_credential_status', { namespace, provider })
}

export async function readCredential(
  namespace: 'stt' | 'llm',
  provider: string,
): Promise<string | null> {
  return invoke('read_credential', { namespace, provider })
}

export async function setCredential(
  namespace: 'stt' | 'llm',
  provider: string,
  value: string,
): Promise<void> {
  return invoke('set_credential', { namespace, provider, value })
}

export async function clearCredential(namespace: 'stt' | 'llm', provider: string): Promise<void> {
  return invoke('clear_credential', { namespace, provider })
}

export async function migrateLegacyCredentials(): Promise<void> {
  return invoke('migrate_legacy_credentials')
}

export async function setCapsuleAutoHide(enabled: boolean): Promise<void> {
  return invoke('set_capsule_auto_hide', { enabled })
}

export async function getPlatformCapabilities(): Promise<PlatformCapabilities> {
  return invoke('get_platform_capabilities')
}

export async function getHotkeyRegistrationError(): Promise<string | null> {
  return invoke('get_hotkey_registration_error')
}

export interface HotkeyBindingStatus {
  value: string
  valid: boolean
}

export type HotkeyAdapter = 'tauriGlobalShortcut' | 'nativeHook' | 'unavailable'
export type HotkeyInstallState = 'starting' | 'installed' | 'failed' | 'disabled'
export type HotkeyRole =
  | 'dictation'
  | 'ask'
  | 'translate'
  | 'editSelection'
  | 'switchScene'
  | 'openApp'

export interface HotkeyStatusError {
  code: string
  message: string
}

export interface HotkeyRoleStatus {
  role: HotkeyRole | string
  adapter: HotkeyAdapter
  state: HotkeyInstallState
  message: string | null
  lastError: HotkeyStatusError | null
}

export interface HotkeyCapability {
  platform: 'macos' | 'windows' | 'linux' | 'unknown' | string
  sessionType: 'wayland' | 'x11' | 'unknown' | string
  supportsGlobalHotkey: boolean
  supportsHoldMode: boolean
  supportsReleasedEdge: boolean
  supportsSideSpecificModifiers: boolean
  requiresAccessibilityPermission: boolean
  statusHint: string | null
}

export interface HotkeyStatus {
  dictation: HotkeyBindingStatus
  ask: HotkeyBindingStatus
  conflict: boolean
  registration_error: string | null
  roles: HotkeyRoleStatus[]
  capability: HotkeyCapability
}

export async function getHotkeyStatus(): Promise<HotkeyStatus> {
  return invoke('get_hotkey_status')
}

export type DiagnosticStatus = 'ok' | 'warning' | 'error' | 'notApplicable' | 'checking'

export interface DiagnosticRow {
  id: 'microphone' | 'accessibility' | 'hotkey' | 'clipboard' | 'insertion' | 'platform' | string
  status: DiagnosticStatus
  message: string
  action: string | null
  lastCheckedAt: string
}

export interface SystemDiagnosticsReport {
  checkedAt: string
  rows: DiagnosticRow[]
}

export async function getSystemDiagnostics(): Promise<SystemDiagnosticsReport> {
  return invoke('get_system_diagnostics')
}

export interface SttProviderDiagnosticIssue {
  code: string
  message: string
}

export interface SttProviderDiagnostics {
  provider: string
  kind: 'localCompatible' | 'builtinLocal' | 'byokRemote' | 'cloudManaged' | 'unknown'
  endpoint: string | null
  model: string | null
  requiresApiKey: boolean
  apiKeyConfigured: boolean
  ready: boolean
  issues: SttProviderDiagnosticIssue[]
}

export async function getSttProviderDiagnostics(
  apiKey: string,
  provider: string,
  customBaseUrl?: string,
  customModel?: string,
): Promise<SttProviderDiagnostics> {
  return invoke('get_stt_provider_diagnostics', {
    apiKey,
    provider,
    customBaseUrl,
    customModel,
  })
}

// Connection test
export async function testSttConnection(
  apiKey: string,
  provider: string,
  customBaseUrl?: string,
  customModel?: string,
  volcengineResourceId?: string,
): Promise<boolean> {
  return invoke('test_stt_connection', {
    apiKey,
    provider,
    customBaseUrl,
    customModel,
    volcengineResourceId,
  })
}

export async function testLlmConnection(
  apiKey: string,
  provider: string,
  baseUrl: string,
  model: string,
): Promise<boolean> {
  return invoke('test_llm_connection', { apiKey, provider, baseUrl, model })
}

// Latency benchmark — returns round-trip time in milliseconds
export async function benchSttConnection(
  apiKey: string,
  provider: string,
  customBaseUrl?: string,
  customModel?: string,
  volcengineResourceId?: string,
): Promise<number> {
  return invoke('bench_stt_connection', {
    apiKey,
    provider,
    customBaseUrl,
    customModel,
    volcengineResourceId,
  })
}

export async function benchLlmConnection(
  apiKey: string,
  provider: string,
  baseUrl: string,
  model: string,
): Promise<number> {
  return invoke('bench_llm_connection', { apiKey, provider, baseUrl, model })
}

// LLM models
export async function fetchLlmModels(apiKey: string, baseUrl: string): Promise<string[]> {
  return invoke('fetch_llm_models', { apiKey, baseUrl })
}

// Hotkey
export async function updateHotkey(hotkey: string): Promise<void> {
  return invoke('update_hotkey', { hotkey })
}

export async function updateAskHotkey(hotkey: string): Promise<void> {
  return invoke('update_ask_hotkey', { hotkey })
}

export async function pauseHotkey(): Promise<void> {
  return invoke('pause_hotkey')
}

export async function resumeHotkey(): Promise<void> {
  return invoke('resume_hotkey')
}

// Ask Anything
export async function askAnything(question: string): Promise<string> {
  return invoke('ask_anything', { question: question.trim() })
}

export async function showAskWindow(): Promise<void> {
  return invoke('show_ask_window')
}

export async function startAskFlow(): Promise<void> {
  return invoke('start_ask_flow')
}

export interface AskDictationResult {
  question: string
  answer: string
  intent: string
  output: 'popupAnswer' | 'openedSearch'
  usedSelectedText: boolean
  selectedTextTruncated: boolean
  searchProvider: string | null
  searchUrl: string | null
}

export interface AskDictationStartResult {
  usedSelectedText: boolean
  selectedTextTruncated: boolean
}

export type PendingAskMessage =
  | { kind: 'result'; payload: AskDictationResult }
  | { kind: 'recordingStarted'; payload: AskDictationStartResult }
  | { kind: 'error'; payload: string }

export async function startAskDictation(): Promise<AskDictationStartResult> {
  return invoke('start_ask_dictation')
}

export async function stopAskDictation(): Promise<AskDictationResult> {
  return invoke('stop_ask_dictation')
}

export async function stopAskFlow(): Promise<void> {
  return invoke('stop_ask_flow')
}

export async function abortAskDictation(): Promise<void> {
  return invoke('abort_ask_dictation')
}

export async function takePendingAskMessage(): Promise<PendingAskMessage | null> {
  return invoke('take_pending_ask_message')
}

// History
export async function getHistory(limit: number, offset: number): Promise<HistoryEntry[]> {
  return invoke('get_history', { limit, offset })
}

export async function clearHistory(): Promise<void> {
  return invoke('clear_history')
}

// Dictionary
export async function getDictionary(): Promise<DictionaryEntry[]> {
  return invoke('get_dictionary')
}

export async function addDictionaryEntry(
  word: string,
  pronunciation: string | null,
): Promise<void> {
  return invoke('add_dictionary_entry', { word, pronunciation })
}

export async function removeDictionaryEntry(id: number): Promise<void> {
  return invoke('remove_dictionary_entry', { id })
}

export async function getCorrectionRules(): Promise<CorrectionRule[]> {
  return invoke('get_correction_rules')
}

export async function addCorrectionRule(pattern: string, replacement: string): Promise<void> {
  return invoke('add_correction_rule', { pattern, replacement })
}

export async function removeCorrectionRule(id: number): Promise<void> {
  return invoke('remove_correction_rule', { id })
}

export async function setCorrectionRuleEnabled(id: number, enabled: boolean): Promise<void> {
  return invoke('set_correction_rule_enabled', { id, enabled })
}

// Auto-start
export async function setAutoStart(enabled: boolean): Promise<void> {
  return invoke('set_auto_start', { enabled })
}

// macOS Accessibility permission
export async function checkAccessibilityPermission(): Promise<boolean> {
  return invoke('check_accessibility_permission')
}

export async function requestAccessibilityPermission(): Promise<boolean> {
  return invoke('request_accessibility_permission')
}

export async function waitForAccessibilityPermission({
  timeoutMs = 60_000,
  intervalMs = 1_000,
}: {
  timeoutMs?: number
  intervalMs?: number
} = {}): Promise<boolean> {
  const deadline = Date.now() + timeoutMs

  while (true) {
    const trusted = await checkAccessibilityPermission()
    if (trusted || Date.now() >= deadline) return trusted
    await new Promise((resolve) => setTimeout(resolve, intervalMs))
  }
}

// Onboarding persistence via tauri-plugin-store
export async function loadOnboardingCompleted(): Promise<boolean> {
  try {
    const { load } = await import('@tauri-apps/plugin-store')
    const store = await load('settings.json')
    const val = await store.get<boolean>('onboarding_completed')
    return val === true
  } catch {
    return false
  }
}

export async function saveOnboardingCompleted(): Promise<void> {
  try {
    const { load } = await import('@tauri-apps/plugin-store')
    const store = await load('settings.json')
    await store.set('onboarding_completed', true)
  } catch (e) {
    console.error('Failed to persist onboarding state:', e)
  }
}
