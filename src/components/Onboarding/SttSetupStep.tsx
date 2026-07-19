import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { testSttConnection } from '../../lib/tauri'
import { CheckCircle2, XCircle, Loader2 } from 'lucide-react'

export function SttSetupStep() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const setSttTestStatus = useAppStore((s) => s.setSttTestStatus)

  // FlowRad build: the transcription provider is fixed.
  useEffect(() => {
    if (config.stt_provider !== 'gemini') {
      updateConfig({ stt_provider: 'gemini' })
      setSttTestStatus('idle')
    }
  }, [config.stt_provider, setSttTestStatus, updateConfig])

  const handleTest = async () => {
    setSttTestStatus('testing')
    try {
      const ok = await testSttConnection(config.stt_api_key, 'gemini')
      setSttTestStatus(ok ? 'success' : 'error')
    } catch {
      setSttTestStatus('error')
    }
  }

  return (
    <div className="space-y-5">
      <Field label={t('onboarding.stt.apiKeyLabel')}>
        <div className="flex gap-2">
          <input
            type="password"
            value={config.stt_api_key}
            onChange={(e) => {
              updateConfig({ stt_api_key: e.target.value })
              setSttTestStatus('idle')
            }}
            placeholder={t('onboarding.stt.apiKeyPlaceholder')}
            className="flex-1 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleTest}
            disabled={sttTestStatus === 'testing' || !config.stt_api_key}
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {sttTestStatus === 'testing' && <Loader2 size={14} className="animate-spin" />}
            {t('onboarding.stt.testButton')}
          </button>
        </div>
        <p className="mt-1.5 text-[11px] text-text-tertiary">{t('onboarding.stt.geminiKeyHint')}</p>
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
