import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import i18n from '../../i18n'
import { ExternalLink } from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useAppStore } from '../../stores/appStore'
import { APP_NAME, APP_VERSION, APP_REPO_URL, UI_LANGUAGES } from '../../lib/constants'
import { SettingSection } from './shared/SettingSection'
import { SettingRow } from './shared/SettingRow'

export function AboutPane() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)

  const currentLang = config.ui_language || i18n.language || 'en'

  const handleSelectLanguage = (value: string) => {
    i18n.changeLanguage(value)
    localStorage.setItem('ui_language', value)
    updateConfig({ ui_language: value })
    invoke('refresh_tray_labels').catch(() => {})
  }

  return (
    <>
      {/* Header */}
      <div className="py-2 text-center">
        <h2 className="text-[22px] font-semibold text-text-primary">{APP_NAME}</h2>
        <p className="mt-1 text-[13px] text-text-secondary">{APP_VERSION}</p>
        <p className="mx-auto mt-3 max-w-[52ch] text-[13px] leading-relaxed text-text-secondary">
          {t('settings.aboutDescription')}
        </p>
      </div>

      {/* Language */}
      <SettingSection title={t('settings.language')}>
        <div className="grid grid-cols-2 gap-3 p-[18px] max-[520px]:grid-cols-1">
          {UI_LANGUAGES.map((lang) => (
            <button
              key={lang.value}
              onClick={() => handleSelectLanguage(lang.value)}
              className={`cursor-pointer rounded-sm border px-4 py-3 text-[13px] transition-all ${
                currentLang === lang.value
                  ? 'border-accent bg-accent/10 font-medium text-accent-text'
                  : 'border-border bg-bg-secondary text-text-primary hover:border-text-tertiary'
              }`}
            >
              <div className="font-medium">{lang.label}</div>
            </button>
          ))}
        </div>
      </SettingSection>

      {/* Open Source */}
      <SettingSection title={t('settings.openSource')}>
        <SettingRow label={t('settings.license')}>
          <span className="text-[13px] text-text-secondary">{t('settings.mit')}</span>
        </SettingRow>
        <SettingRow label={t('settings.github')}>
          <button
            onClick={() => openUrl(APP_REPO_URL)}
            className="flex cursor-pointer items-center gap-1 border-none bg-transparent text-[13px] font-medium text-accent-text hover:underline"
          >
            {t('settings.view')} <ExternalLink size={12} />
          </button>
        </SettingRow>
        <SettingRow label={t('settings.framework')}>
          <span className="text-[13px] text-text-secondary">{t('settings.tauriReact')}</span>
        </SettingRow>
      </SettingSection>
    </>
  )
}
