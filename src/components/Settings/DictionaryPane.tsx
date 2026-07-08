import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Trash2, Plus } from 'lucide-react'
import { useAppStore } from '../../stores/appStore'
import {
  addCorrectionRule,
  addDictionaryEntry,
  getCorrectionRules,
  getDictionary,
  removeCorrectionRule,
  removeDictionaryEntry,
  setCorrectionRuleEnabled,
} from '../../lib/tauri'
import { toast } from '../Toast'

export function DictionaryPane() {
  const dictionary = useAppStore((s) => s.dictionary)
  const setDictionary = useAppStore((s) => s.setDictionary)
  const correctionRules = useAppStore((s) => s.correctionRules)
  const setCorrectionRules = useAppStore((s) => s.setCorrectionRules)
  const { t } = useTranslation()
  const [word, setWord] = useState('')
  const [pronunciation, setPronunciation] = useState('')
  const [pattern, setPattern] = useState('')
  const [replacement, setReplacement] = useState('')

  const handleAdd = async () => {
    if (!word.trim()) return
    try {
      await addDictionaryEntry(word.trim(), pronunciation.trim() || null)
      setWord('')
      setPronunciation('')
      const updated = await getDictionary()
      setDictionary(updated)
    } catch (e) {
      console.error('Failed to add entry:', e)
      toast.error(t('dictionary.failedToAdd'))
    }
  }

  const handleRemove = async (id: number) => {
    try {
      await removeDictionaryEntry(id)
      const updated = await getDictionary()
      setDictionary(updated)
    } catch (e) {
      console.error('Failed to remove entry:', e)
      toast.error(t('dictionary.failedToRemove'))
    }
  }

  const handleAddCorrection = async () => {
    const nextPattern = pattern.trim()
    const nextReplacement = replacement.trim()
    if (!nextPattern || !nextReplacement) return
    try {
      await addCorrectionRule(nextPattern, nextReplacement)
      setPattern('')
      setReplacement('')
      const updated = await getCorrectionRules()
      setCorrectionRules(updated)
    } catch (e) {
      console.error('Failed to add correction rule:', e)
      toast.error(t('dictionary.failedToAddCorrection'))
    }
  }

  const handleRemoveCorrection = async (id: number) => {
    try {
      await removeCorrectionRule(id)
      const updated = await getCorrectionRules()
      setCorrectionRules(updated)
    } catch (e) {
      console.error('Failed to remove correction rule:', e)
      toast.error(t('dictionary.failedToRemoveCorrection'))
    }
  }

  const handleToggleCorrection = async (id: number, enabled: boolean) => {
    try {
      await setCorrectionRuleEnabled(id, enabled)
      const updated = await getCorrectionRules()
      setCorrectionRules(updated)
    } catch (e) {
      console.error('Failed to update correction rule:', e)
      toast.error(t('dictionary.failedToUpdateCorrection'))
    }
  }

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
        <input
          value={word}
          onChange={(e) => setWord(e.target.value)}
          placeholder={t('dictionary.word')}
          className="min-w-0 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        />
        <input
          value={pronunciation}
          onChange={(e) => setPronunciation(e.target.value)}
          placeholder={t('dictionary.pronunciationOptional')}
          className="min-w-0 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        />
        <button
          onClick={handleAdd}
          disabled={!word.trim()}
          className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-1.5 sm:shrink-0"
        >
          <Plus size={14} />
          {t('dictionary.add')}
        </button>
      </div>

      <div className="border border-border rounded-[10px] overflow-hidden">
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_32px] gap-2 bg-bg-secondary px-3 py-2.5 text-[11px] font-medium uppercase tracking-wider text-text-secondary">
          <span>{t('dictionary.word')}</span>
          <span>{t('dictionary.pronunciation')}</span>
          <span />
        </div>
        {dictionary.length === 0 ? (
          <div className="px-3 py-8 text-center text-[13px] text-text-tertiary">
            {t('dictionary.noEntries')}
          </div>
        ) : (
          dictionary.map((entry) => (
            <div
              key={entry.id}
              className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_32px] gap-2 border-t border-border px-3 py-2.5 text-[13px] transition-colors hover:bg-bg-secondary/50"
            >
              <span className="min-w-0 truncate text-text-primary">{entry.word}</span>
              <span className="min-w-0 truncate text-text-secondary">
                {entry.pronunciation || '-'}
              </span>
              <button
                onClick={() => handleRemove(entry.id)}
                className="rounded-[6px] border-none bg-transparent p-1 text-text-tertiary transition-colors hover:bg-bg-tertiary hover:text-error"
                aria-label={t('dictionary.removeEntry')}
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))
        )}
      </div>

      <div className="space-y-3">
        <h3 className="text-[13px] font-medium text-text-primary">{t('dictionary.corrections')}</h3>

        <div className="grid grid-cols-1 gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
          <input
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
            placeholder={t('dictionary.wrongPhrase')}
            className="min-w-0 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <input
            value={replacement}
            onChange={(e) => setReplacement(e.target.value)}
            placeholder={t('dictionary.correctPhrase')}
            className="min-w-0 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleAddCorrection}
            disabled={!pattern.trim() || !replacement.trim()}
            aria-label={t('dictionary.addCorrection')}
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-1.5 sm:shrink-0"
          >
            <Plus size={14} />
            {t('dictionary.add')}
          </button>
        </div>

        <div className="border border-border rounded-[10px] overflow-hidden">
          {correctionRules.length === 0 ? (
            <div className="px-3 py-8 text-center text-[13px] text-text-tertiary">
              {t('dictionary.noCorrections')}
            </div>
          ) : (
            correctionRules.map((rule) => (
              <div
                key={rule.id}
                className="grid grid-cols-[auto_minmax(0,1fr)_32px] items-center gap-3 border-t first:border-t-0 border-border px-3 py-2.5 text-[13px] transition-colors hover:bg-bg-secondary/50"
              >
                <input
                  type="checkbox"
                  checked={rule.enabled}
                  onChange={(e) => handleToggleCorrection(rule.id, e.target.checked)}
                  aria-label={t('dictionary.toggleCorrection')}
                  className="h-4 w-4 accent-accent"
                />
                <div className="min-w-0">
                  <p className="truncate text-text-primary">{rule.pattern}</p>
                  <p className="truncate text-[12px] text-text-secondary">{rule.replacement}</p>
                </div>
                <button
                  onClick={() => handleRemoveCorrection(rule.id)}
                  className="rounded-[6px] border-none bg-transparent p-1 text-text-tertiary transition-colors hover:bg-bg-tertiary hover:text-error"
                  aria-label={t('dictionary.removeCorrection')}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
