import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Pencil, Trash2, X } from 'lucide-react'
import type { CustomAppMappingView } from '../../lib/tauri'
import {
  deleteCustomAppMapping,
  resetCustomAppMappings,
  setCustomAppMappingEnabled,
} from '../../lib/tauri'
import { AppLogo } from '../AppLogo'

interface ManageAppMappingsDialogProps {
  mappings: CustomAppMappingView[]
  onCancel: () => void
  onChanged: () => void | Promise<void>
  onEdit: (mapping: CustomAppMappingView) => void
}

export function ManageAppMappingsDialog({
  mappings,
  onCancel,
  onChanged,
  onEdit,
}: ManageAppMappingsDialogProps) {
  const { t } = useTranslation()
  const [rows, setRows] = useState(mappings)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [resetConfirming, setResetConfirming] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => setRows(mappings), [mappings])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || busyId) return
      event.preventDefault()
      if (resetConfirming) setResetConfirming(false)
      else onCancel()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [busyId, onCancel, resetConfirming])

  const setEnabled = async (mapping: CustomAppMappingView) => {
    setBusyId(mapping.id)
    setError(null)
    try {
      await setCustomAppMappingEnabled(mapping.id, !mapping.enabled)
      setRows((current) =>
        current.map((row) => (row.id === mapping.id ? { ...row, enabled: !mapping.enabled } : row)),
      )
      await onChanged()
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError))
    } finally {
      setBusyId(null)
    }
  }

  const remove = async (mapping: CustomAppMappingView) => {
    setBusyId(mapping.id)
    setError(null)
    try {
      await deleteCustomAppMapping(mapping.id)
      setRows((current) => current.filter((row) => row.id !== mapping.id))
      await onChanged()
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError))
    } finally {
      setBusyId(null)
    }
  }

  const reset = async () => {
    setBusyId('reset')
    setError(null)
    try {
      await resetCustomAppMappings()
      setRows([])
      setResetConfirming(false)
      await onChanged()
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/25 px-5">
      <div className="fixed inset-0" onClick={busyId ? undefined : onCancel} />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t('settings.manageAppMappingsTitle')}
        className="relative z-10 flex max-h-[72vh] w-full max-w-[460px] flex-col rounded-[10px] border border-border bg-bg-primary shadow-float"
      >
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <h3 className="text-[14px] font-medium text-text-primary">
            {t('settings.manageAppMappingsTitle')}
          </h3>
          <button
            type="button"
            onClick={onCancel}
            disabled={Boolean(busyId)}
            aria-label={t('settings.mappingCancel')}
            className="grid h-7 w-7 place-items-center rounded-[6px] border-none bg-transparent text-text-tertiary hover:bg-bg-hover hover:text-text-primary disabled:opacity-40"
          >
            <X size={15} />
          </button>
        </div>

        <div className="min-h-0 overflow-y-auto px-4">
          {rows.length === 0 ? (
            <p className="py-7 text-center text-[12px] text-text-tertiary">
              {t('settings.mappingNoMappings')}
            </p>
          ) : (
            rows.map((mapping) => (
              <div
                key={mapping.id}
                className="flex min-w-0 items-center gap-2 border-b border-border py-2.5 last:border-b-0"
              >
                <AppLogo iconKey={mapping.iconKey} family={mapping.family} />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[12px] font-medium text-text-primary">
                    {mapping.label}
                  </p>
                  <p className="truncate text-[11px] text-text-tertiary">{mapping.displayValue}</p>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={mapping.enabled}
                  aria-label={t('settings.mappingEnabled')}
                  disabled={busyId === mapping.id}
                  onClick={() => void setEnabled(mapping)}
                  className={`relative h-5 w-9 flex-none rounded-full border-none transition-colors disabled:opacity-40 ${
                    mapping.enabled ? 'bg-accent' : 'bg-border'
                  }`}
                >
                  <span
                    className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow-sm transition-transform ${
                      mapping.enabled ? 'translate-x-[18px]' : 'translate-x-0.5'
                    }`}
                  />
                </button>
                <button
                  type="button"
                  onClick={() => onEdit(mapping)}
                  aria-label={t('settings.mappingEdit')}
                  title={t('settings.mappingEdit')}
                  className="grid h-7 w-7 flex-none place-items-center rounded-[6px] border-none bg-transparent text-text-tertiary hover:bg-bg-hover hover:text-text-primary"
                >
                  <Pencil size={14} />
                </button>
                <button
                  type="button"
                  disabled={busyId === mapping.id}
                  onClick={() => void remove(mapping)}
                  aria-label={t('settings.mappingDelete')}
                  title={t('settings.mappingDelete')}
                  className="grid h-7 w-7 flex-none place-items-center rounded-[6px] border-none bg-transparent text-text-tertiary hover:bg-bg-hover hover:text-error disabled:opacity-40"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))
          )}
        </div>

        {error && <p className="px-4 pb-2 text-[11px] text-error">{error}</p>}

        <div className="border-t border-border px-4 py-3">
          {resetConfirming ? (
            <div className="flex flex-wrap items-center justify-end gap-2">
              <p className="mr-auto text-[11px] text-text-secondary">
                {t('settings.mappingResetConfirm')}
              </p>
              <button
                type="button"
                onClick={() => setResetConfirming(false)}
                disabled={Boolean(busyId)}
                className="rounded-[8px] border border-border bg-transparent px-3 py-1.5 text-[12px] text-text-secondary disabled:opacity-40"
              >
                {t('settings.mappingCancel')}
              </button>
              <button
                type="button"
                onClick={() => void reset()}
                disabled={Boolean(busyId)}
                className="rounded-[8px] border-none bg-error px-3 py-1.5 text-[12px] text-white disabled:opacity-40"
              >
                {t('settings.mappingResetConfirmAction')}
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setResetConfirming(true)}
              disabled={rows.length === 0 || Boolean(busyId)}
              className="text-[12px] text-text-tertiary hover:text-error disabled:opacity-40"
            >
              {t('settings.mappingReset')}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
