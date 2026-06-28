import { useEffect, useState } from 'react'
import { Download, RefreshCw, X } from 'lucide-react'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { useTranslation } from 'react-i18next'

type InstallState = 'idle' | 'installing' | 'error'

export function UpdatePrompt() {
  const { t } = useTranslation()
  const [update, setUpdate] = useState<Update | null>(null)
  const [dismissed, setDismissed] = useState(false)
  const [installState, setInstallState] = useState<InstallState>('idle')

  useEffect(() => {
    let cancelled = false

    check()
      .then((availableUpdate) => {
        if (!cancelled && availableUpdate) {
          setUpdate(availableUpdate)
        }
      })
      .catch((error) => {
        console.warn('Failed to check for updates:', error)
      })

    return () => {
      cancelled = true
    }
  }, [])

  if (!update || dismissed) return null

  const installUpdate = async () => {
    setInstallState('installing')
    try {
      await update.downloadAndInstall()
      await relaunch()
    } catch (error) {
      console.warn('Failed to install update:', error)
      setInstallState('error')
    }
  }

  const installing = installState === 'installing'

  return (
    <div
      className="fixed right-4 bottom-4 z-[9998] w-[min(360px,calc(100vw-32px))] rounded-[8px] border border-border bg-bg-secondary p-4 shadow-xl"
      role="status"
      aria-live="polite"
    >
      <div className="flex items-start gap-3">
        <div className="mt-0.5 flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[8px] bg-accent/10 text-accent">
          <Download size={17} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-3">
            <div>
              <h2 className="text-[14px] font-semibold text-text-primary">
                {t('updates.availableTitle', 'Update available')}
              </h2>
              <p className="mt-1 text-[12px] leading-5 text-text-secondary">
                {t('updates.availableBody', 'Version {{version}} is ready to install.', {
                  version: update.version,
                })}
              </p>
            </div>
            <button
              type="button"
              onClick={() => setDismissed(true)}
              className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-[6px] border border-transparent text-text-tertiary transition-colors hover:border-border hover:text-text-primary"
              aria-label={t('updates.dismiss', 'Dismiss')}
            >
              <X size={15} />
            </button>
          </div>

          {installState === 'error' && (
            <p className="mt-3 text-[12px] leading-5 text-error">
              {t(
                'updates.error',
                'Update failed. Please download the latest version from the website.',
              )}
            </p>
          )}

          <button
            type="button"
            onClick={installUpdate}
            disabled={installing}
            className="mt-3 inline-flex h-9 items-center justify-center gap-2 rounded-[7px] bg-accent px-3 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-70"
          >
            {installing ? <RefreshCw size={14} className="animate-spin" /> : <Download size={14} />}
            {installing
              ? t('updates.installing', 'Installing...')
              : t('updates.install', 'Install update')}
          </button>
        </div>
      </div>
    </div>
  )
}
