import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore, type AppConfig } from '../../stores/appStore'
import { CUSTOM_WHISPER_PROVIDER, ONBOARDING_STT_PROVIDERS } from '../../lib/constants'
import { testSttConnection } from '../../lib/tauri'
import { CheckCircle2, XCircle, Loader2 } from 'lucide-react'

export function SttSetupStep() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const setSttTestStatus = useAppStore((s) => s.setSttTestStatus)
  const fallbackProvider = ONBOARDING_STT_PROVIDERS[0]?.value ?? 'deepgram'
  const isCustomWhisper = config.stt_provider === CUSTOM_WHISPER_PROVIDER
  const selectedProvider = isCustomWhisper
    ? CUSTOM_WHISPER_PROVIDER
    : ONBOARDING_STT_PROVIDERS.some((p) => p.value === config.stt_provider)
      ? config.stt_provider
      : fallbackProvider

  useEffect(() => {
    if (isCustomWhisper) return
    if (selectedProvider === config.stt_provider) return

    updateConfig({ stt_provider: selectedProvider as AppConfig['stt_provider'] })
    setSttTestStatus('idle')
  }, [config.stt_provider, isCustomWhisper, selectedProvider, setSttTestStatus, updateConfig])

  const handleTest = async () => {
    setSttTestStatus('testing')
    try {
      const ok = isCustomWhisper
        ? await testSttConnection(
            config.stt_custom_api_key,
            CUSTOM_WHISPER_PROVIDER,
            config.stt_custom_base_url,
            config.stt_custom_model,
          )
        : await testSttConnection(config.stt_api_key, selectedProvider)
      setSttTestStatus(ok ? 'success' : 'error')
    } catch {
      setSttTestStatus('error')
    }
  }

  return (
    <div className="space-y-5">
      {isCustomWhisper ? (
        <Field label={t('onboarding.stt.serviceLabel')}>
          <div className="rounded-[10px] border border-border bg-bg-secondary px-3 py-2.5">
            <p className="text-[13px] font-medium text-text-primary">
              {t('onboarding.stt.customWhisperConfigured')}
            </p>
            <p className="mt-1 truncate text-[11px] text-text-tertiary">
              {config.stt_custom_base_url}
            </p>
            <p className="mt-0.5 truncate text-[11px] text-text-tertiary">
              {config.stt_custom_model}
            </p>
          </div>
        </Field>
      ) : (
        <Field label={t('onboarding.stt.serviceLabel')}>
          <select
            value={selectedProvider}
            onChange={(e) => {
              updateConfig({ stt_provider: e.target.value as AppConfig['stt_provider'] })
              setSttTestStatus('idle')
            }}
            className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          >
            {ONBOARDING_STT_PROVIDERS.map((p) => (
              <option key={p.value} value={p.value}>
                {t(p.labelKey)}
              </option>
            ))}
          </select>
        </Field>
      )}

      <Field label={t('onboarding.stt.apiKeyLabel')}>
        <div className="flex gap-2">
          <input
            type="password"
            value={isCustomWhisper ? config.stt_custom_api_key : config.stt_api_key}
            onChange={(e) => {
              updateConfig(
                isCustomWhisper
                  ? { stt_custom_api_key: e.target.value }
                  : { stt_api_key: e.target.value },
              )
              setSttTestStatus('idle')
            }}
            placeholder={t('onboarding.stt.apiKeyPlaceholder')}
            className="flex-1 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleTest}
            disabled={
              sttTestStatus === 'testing' ||
              (isCustomWhisper
                ? !config.stt_custom_base_url.trim() || !config.stt_custom_model.trim()
                : !config.stt_api_key)
            }
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {sttTestStatus === 'testing' && <Loader2 size={14} className="animate-spin" />}
            {t('onboarding.stt.testButton')}
          </button>
        </div>
        <TestStatusHint status={sttTestStatus} />
      </Field>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-[13px] font-medium text-text-secondary mb-2">{label}</label>
      {children}
    </div>
  )
}

function TestStatusHint({ status }: { status: string }) {
  const { t } = useTranslation()
  if (status === 'success') {
    return (
      <p className="flex items-center gap-1 text-[12px] text-success mt-2">
        <CheckCircle2 size={13} /> {t('onboarding.stt.connectionOk')}
      </p>
    )
  }
  if (status === 'error') {
    return (
      <p className="flex items-center gap-1 text-[12px] text-error mt-2">
        <XCircle size={13} /> {t('onboarding.stt.connectionFail')}
      </p>
    )
  }
  return null
}
