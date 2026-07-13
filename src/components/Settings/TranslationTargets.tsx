import { useEffect, useState } from 'react'
import { ArrowDown, ArrowUp, ChevronDown, Plus, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { TARGET_LANGUAGES } from '../../lib/constants'
import type { TranslationConfig } from '../../stores/appStore'

interface TranslationTargetsProps {
  value: TranslationConfig
  onChange: (value: TranslationConfig) => void
}

const MAX_TRANSLATION_TARGETS = 5

export function TranslationTargets({ value, onChange }: TranslationTargetsProps) {
  const { t } = useTranslation()
  const [managing, setManaging] = useState(false)
  const selected = new Set(value.targets)
  const multiple = value.targets.length > 1

  useEffect(() => {
    if (!multiple) setManaging(false)
  }, [multiple])

  const updateTarget = (index: number, code: string) => {
    if (selected.has(code) && value.targets[index] !== code) return
    const previous = value.targets[index]
    const targets = [...value.targets]
    targets[index] = code
    onChange({
      targets,
      active_target: value.active_target === previous ? code : value.active_target,
    })
  }

  const moveTarget = (index: number, offset: -1 | 1) => {
    const destination = index + offset
    if (destination < 0 || destination >= value.targets.length) return
    const targets = [...value.targets]
    ;[targets[index], targets[destination]] = [targets[destination], targets[index]]
    onChange({ targets, active_target: value.active_target })
  }

  const removeTarget = (index: number) => {
    if (value.targets.length === 1) return
    const removed = value.targets[index]
    const targets = value.targets.filter((_, targetIndex) => targetIndex !== index)
    const activeTarget =
      removed === value.active_target
        ? targets[Math.min(index, targets.length - 1)]
        : value.active_target
    onChange({ targets, active_target: activeTarget })
  }

  const addTarget = () => {
    if (value.targets.length >= MAX_TRANSLATION_TARGETS) return
    const next = TARGET_LANGUAGES.find((language) => !selected.has(language.value))
    if (!next) return
    onChange({
      targets: [...value.targets, next.value],
      active_target: value.active_target,
    })
  }

  const selectCompactTarget = (code: string) => {
    if (multiple) {
      onChange({ ...value, active_target: code })
      return
    }
    updateTarget(0, code)
  }

  return (
    <div className="space-y-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="flex-none text-[11px] text-text-tertiary">
          {t('settings.targetLanguage')}
        </span>
        <select
          value={value.active_target}
          onChange={(event) => selectCompactTarget(event.target.value)}
          aria-label={t('settings.targetLanguage')}
          className="h-8 min-w-0 flex-1 rounded-[8px] border border-border bg-bg-secondary px-2 text-[12px] text-text-primary outline-none transition-colors focus:border-border-focus"
        >
          {TARGET_LANGUAGES.filter((language) =>
            multiple ? selected.has(language.value) : true,
          ).map((language) => (
            <option key={language.value} value={language.value}>
              {language.labelKey ? t(language.labelKey) : language.label}
            </option>
          ))}
        </select>
        {multiple && (
          <button
            type="button"
            onClick={() => setManaging((current) => !current)}
            aria-expanded={managing}
            aria-label={t('settings.manageTranslationTargets')}
            title={t('settings.manageTranslationTargets')}
            className="flex h-8 flex-none items-center gap-1 rounded-[6px] border border-border bg-transparent px-2 text-[11px] text-text-secondary hover:border-border-focus hover:text-text-primary"
          >
            {t('settings.manageTranslationTargets')}
            <ChevronDown
              size={12}
              className={`transition-transform ${managing ? 'rotate-180' : ''}`}
            />
          </button>
        )}
        <button
          type="button"
          onClick={addTarget}
          disabled={value.targets.length >= MAX_TRANSLATION_TARGETS}
          aria-label={t('settings.addTranslationTarget')}
          title={t('settings.addTranslationTarget')}
          className="flex h-8 w-8 flex-none items-center justify-center rounded-[6px] border border-border bg-bg-secondary text-text-secondary transition-colors hover:border-border-focus hover:text-text-primary disabled:cursor-default disabled:opacity-40"
        >
          <Plus size={13} />
        </button>
      </div>

      {managing && multiple && (
        <div className="divide-y divide-border border-y border-border">
          {value.targets.map((code, index) => (
            <div
              key={code}
              data-testid={`translation-target-${code}`}
              className="flex h-10 items-center gap-1.5"
            >
              <select
                value={code}
                onChange={(event) => updateTarget(index, event.target.value)}
                aria-label={`${t('settings.translationTarget')} ${index + 1}`}
                className="h-8 min-w-0 flex-1 rounded-[8px] border border-border bg-bg-secondary px-2 text-[12px] text-text-primary outline-none transition-colors focus:border-border-focus"
              >
                {TARGET_LANGUAGES.filter(
                  (language) => language.value === code || !selected.has(language.value),
                ).map((language) => (
                  <option key={language.value} value={language.value}>
                    {language.labelKey ? t(language.labelKey) : language.label}
                  </option>
                ))}
              </select>
              <button
                type="button"
                onClick={() => moveTarget(index, -1)}
                disabled={index === 0}
                aria-label={`${t('settings.moveTranslationTargetUp')} ${code}`}
                title={t('settings.moveTranslationTargetUp')}
                className="flex h-7 w-7 flex-none items-center justify-center rounded-[6px] border-none bg-transparent text-text-tertiary transition-colors hover:bg-bg-tertiary hover:text-text-primary disabled:cursor-default disabled:opacity-30"
              >
                <ArrowUp size={13} />
              </button>
              <button
                type="button"
                onClick={() => moveTarget(index, 1)}
                disabled={index === value.targets.length - 1}
                aria-label={`${t('settings.moveTranslationTargetDown')} ${code}`}
                title={t('settings.moveTranslationTargetDown')}
                className="flex h-7 w-7 flex-none items-center justify-center rounded-[6px] border-none bg-transparent text-text-tertiary transition-colors hover:bg-bg-tertiary hover:text-text-primary disabled:cursor-default disabled:opacity-30"
              >
                <ArrowDown size={13} />
              </button>
              <button
                type="button"
                onClick={() => removeTarget(index)}
                aria-label={`${t('settings.removeTranslationTarget')} ${code}`}
                title={t('settings.removeTranslationTarget')}
                className="flex h-7 w-7 flex-none items-center justify-center rounded-[6px] border-none bg-transparent text-text-tertiary transition-colors hover:bg-red-500/10 hover:text-red-500"
              >
                <Trash2 size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
