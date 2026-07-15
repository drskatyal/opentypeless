import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { LLM_DEFAULT_CONFIG } from '../../lib/constants'
import { testLlmConnection } from '../../lib/tauri'
import { CheckCircle2, XCircle, Loader2 } from 'lucide-react'

export function LlmSetupStep() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const llmTestStatus = useAppStore((s) => s.llmTestStatus)
  const setLlmTestStatus = useAppStore((s) => s.setLlmTestStatus)

  // Gemini-only build: provider, base URL and model are fixed to Gemini defaults.
  useEffect(() => {
    if (config.llm_provider !== 'gemini') {
      const defaults = LLM_DEFAULT_CONFIG.gemini
      updateConfig({
        llm_provider: 'gemini',
        llm_base_url: defaults.baseUrl,
        llm_model: defaults.model,
      })
      setLlmTestStatus('idle')
    }
  }, [config.llm_provider, setLlmTestStatus, updateConfig])

  const handleTest = async () => {
    setLlmTestStatus('testing')
    try {
      const ok = await testLlmConnection(
        config.llm_api_key,
        'gemini',
        config.llm_base_url,
        config.llm_model,
      )
      setLlmTestStatus(ok ? 'success' : 'error')
    } catch {
      setLlmTestStatus('error')
    }
  }

  return (
    <div className="space-y-5">
      <Field label={t('onboarding.llm.apiKeyLabel')}>
        <div className="flex gap-2">
          <input
            type="password"
            value={config.llm_api_key}
            onChange={(e) => {
              updateConfig({ llm_api_key: e.target.value })
              setLlmTestStatus('idle')
            }}
            placeholder={t('onboarding.llm.apiKeyPlaceholder')}
            className="flex-1 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleTest}
            disabled={!config.llm_api_key || llmTestStatus === 'testing'}
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {llmTestStatus === 'testing' && <Loader2 size={14} className="animate-spin" />}
            {t('onboarding.llm.testButton')}
          </button>
        </div>
        <p className="mt-1.5 text-[11px] text-text-tertiary">{t('onboarding.llm.geminiKeyHint')}</p>
        <TestStatusHint status={llmTestStatus} />
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
        <CheckCircle2 size={13} /> {t('onboarding.llm.connectionOk')}
      </p>
    )
  }
  if (status === 'error') {
    return (
      <p className="flex items-center gap-1 text-[12px] text-error mt-2">
        <XCircle size={13} /> {t('onboarding.llm.connectionFail')}
      </p>
    )
  }
  return null
}
