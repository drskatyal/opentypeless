import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Check,
  Download,
  FileJson,
  FileSpreadsheet,
  Pencil,
  Plus,
  Search,
  Trash2,
  Upload,
  X,
} from 'lucide-react'
import { useAppStore } from '../../stores/appStore'
import {
  addCorrectionRule,
  addDictionaryEntry,
  commitDictionaryImport,
  exportDictionaryCsv,
  exportDictionaryJson,
  getCorrectionRules,
  getDictionary,
  previewDictionaryImport,
  removeCorrectionRule,
  removeDictionaryEntry,
  setCorrectionRuleEnabled,
  updateCorrectionRule,
  updateDictionaryEntry,
  type DictionaryImportFormat,
  type DictionaryImportReport,
} from '../../lib/tauri'
import { toast } from '../toast-service'
import { DictionaryImportDialog } from './DictionaryImportDialog'
import { SegmentedControl } from './shared/SegmentedControl'

const MAX_IMPORT_BYTES = 1024 * 1024

type EditingRow =
  | { kind: 'dictionary'; id: number; word: string; pronunciation: string }
  | {
      kind: 'correction'
      id: number
      pattern: string
      replacement: string
      enabled: boolean
    }
  | null

interface PendingImport {
  fileName: string
  bytes: number[]
  format: DictionaryImportFormat
  report: DictionaryImportReport
}

function importFormat(fileName: string): DictionaryImportFormat | null {
  const extension = fileName.split('.').pop()?.toLowerCase()
  return extension === 'txt' || extension === 'csv' || extension === 'json' ? extension : null
}

function downloadText(content: string, format: 'json' | 'csv') {
  const type = format === 'json' ? 'application/json' : 'text/csv;charset=utf-8'
  const blob = new Blob([content], { type })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `opentypeless-dictionary-${new Date().toISOString().slice(0, 10)}.${format}`
  link.click()
  URL.revokeObjectURL(url)
}

export function DictionaryPane() {
  const dictionary = useAppStore((state) => state.dictionary)
  const setDictionary = useAppStore((state) => state.setDictionary)
  const correctionRules = useAppStore((state) => state.correctionRules)
  const setCorrectionRules = useAppStore((state) => state.setCorrectionRules)
  const { t } = useTranslation()
  const [activeSection, setActiveSection] = useState<'words' | 'corrections'>('words')
  const [word, setWord] = useState('')
  const [pronunciation, setPronunciation] = useState('')
  const [pattern, setPattern] = useState('')
  const [replacement, setReplacement] = useState('')
  const [search, setSearch] = useState('')
  const [editing, setEditing] = useState<EditingRow>(null)
  const [exportMenuOpen, setExportMenuOpen] = useState(false)
  const [pendingImport, setPendingImport] = useState<PendingImport | null>(null)
  const [committingImport, setCommittingImport] = useState(false)
  const importInputRef = useRef<HTMLInputElement>(null)
  const importButtonRef = useRef<HTMLButtonElement>(null)
  const exportButtonRef = useRef<HTMLButtonElement>(null)

  const query = search.trim().toLocaleLowerCase()
  const filteredDictionary = useMemo(
    () =>
      query
        ? dictionary.filter((entry) =>
            `${entry.word}\n${entry.pronunciation ?? ''}`.toLocaleLowerCase().includes(query),
          )
        : dictionary,
    [dictionary, query],
  )
  const filteredCorrections = useMemo(
    () =>
      query
        ? correctionRules.filter((rule) =>
            `${rule.pattern}\n${rule.replacement}`.toLocaleLowerCase().includes(query),
          )
        : correctionRules,
    [correctionRules, query],
  )

  const refreshDictionary = useCallback(async () => {
    const [nextDictionary, nextCorrections] = await Promise.all([
      getDictionary(),
      getCorrectionRules(),
    ])
    setDictionary(nextDictionary)
    setCorrectionRules(nextCorrections)
  }, [setCorrectionRules, setDictionary])

  const closeExportMenu = useCallback(() => {
    setExportMenuOpen(false)
    window.setTimeout(() => exportButtonRef.current?.focus(), 0)
  }, [])

  useEffect(() => {
    if (!exportMenuOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      closeExportMenu()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [closeExportMenu, exportMenuOpen])

  const handleAdd = async () => {
    if (!word.trim()) return
    try {
      await addDictionaryEntry(word.trim(), pronunciation.trim() || null)
      setWord('')
      setPronunciation('')
      await refreshDictionary()
    } catch (error) {
      console.error('Failed to add entry:', error)
      toast.error(t('dictionary.failedToAdd'))
    }
  }

  const handleRemove = async (id: number) => {
    try {
      await removeDictionaryEntry(id)
      await refreshDictionary()
    } catch (error) {
      console.error('Failed to remove entry:', error)
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
      await refreshDictionary()
    } catch (error) {
      console.error('Failed to add correction rule:', error)
      toast.error(t('dictionary.failedToAddCorrection'))
    }
  }

  const handleRemoveCorrection = async (id: number) => {
    try {
      await removeCorrectionRule(id)
      await refreshDictionary()
    } catch (error) {
      console.error('Failed to remove correction rule:', error)
      toast.error(t('dictionary.failedToRemoveCorrection'))
    }
  }

  const handleToggleCorrection = async (id: number, enabled: boolean) => {
    try {
      await setCorrectionRuleEnabled(id, enabled)
      await refreshDictionary()
    } catch (error) {
      console.error('Failed to update correction rule:', error)
      toast.error(t('dictionary.failedToUpdateCorrection'))
    }
  }

  const saveEdit = async () => {
    if (!editing) return
    try {
      if (editing.kind === 'dictionary') {
        const nextWord = editing.word.trim()
        if (!nextWord) return
        await updateDictionaryEntry(editing.id, nextWord, editing.pronunciation.trim() || null)
      } else {
        const nextPattern = editing.pattern.trim()
        const nextReplacement = editing.replacement.trim()
        if (!nextPattern || !nextReplacement) return
        await updateCorrectionRule(editing.id, nextPattern, nextReplacement, editing.enabled)
      }
      setEditing(null)
      await refreshDictionary()
    } catch (error) {
      console.error('Failed to edit dictionary row:', error)
      toast.error(t('dictionary.failedToUpdate'))
    }
  }

  const handleImportFile = async (file: File | undefined) => {
    if (!file) return
    const format = importFormat(file.name)
    if (!format) {
      toast.error(t('dictionary.importUnsupported'))
      return
    }
    if (file.size > MAX_IMPORT_BYTES) {
      toast.error(t('dictionary.importTooLarge'))
      return
    }
    try {
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
      const report = await previewDictionaryImport(bytes, format)
      setPendingImport({ fileName: file.name, bytes, format, report })
    } catch (error) {
      console.error('Failed to preview dictionary import:', error)
      toast.error(t('dictionary.importFailed'))
    }
  }

  const closeImport = useCallback(() => {
    setPendingImport(null)
    window.setTimeout(() => importButtonRef.current?.focus(), 0)
  }, [])

  const confirmImport = async () => {
    if (!pendingImport) return
    setCommittingImport(true)
    try {
      const report = await commitDictionaryImport(pendingImport.bytes, pendingImport.format)
      await refreshDictionary()
      setPendingImport(null)
      toast.success(t('dictionary.importComplete', { count: report.accepted }))
    } catch (error) {
      console.error('Failed to import dictionary:', error)
      toast.error(t('dictionary.importFailed'))
    } finally {
      setCommittingImport(false)
    }
  }

  const exportContent = async (format: 'json' | 'csv') => {
    try {
      const content = format === 'json' ? await exportDictionaryJson() : await exportDictionaryCsv()
      downloadText(content, format)
      closeExportMenu()
    } catch (error) {
      console.error('Failed to export dictionary:', error)
      toast.error(t('dictionary.exportFailed'))
    }
  }

  return (
    <div className="space-y-5">
      <div className="flex min-w-0 items-center gap-2">
        <div className="relative min-w-0 flex-1">
          <Search
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-text-tertiary"
          />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('dictionary.searchPlaceholder')}
            className="h-9 w-full min-w-0 rounded-[8px] border border-border bg-bg-secondary pl-8 pr-3 text-[12px] text-text-primary outline-none focus:border-border-focus"
          />
        </div>
        <button
          ref={importButtonRef}
          type="button"
          onClick={() => importInputRef.current?.click()}
          aria-label={t('dictionary.import')}
          title={t('dictionary.import')}
          className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[6px] border border-border bg-transparent text-text-secondary hover:border-border-focus hover:text-text-primary"
        >
          <Upload size={14} />
        </button>
        <input
          ref={importInputRef}
          type="file"
          accept=".txt,.csv,.json,text/plain,text/csv,application/json"
          aria-label={t('dictionary.importFile')}
          className="hidden"
          onChange={(event) => {
            const file = event.target.files?.[0]
            event.target.value = ''
            void handleImportFile(file)
          }}
        />
        <div className="relative">
          <button
            ref={exportButtonRef}
            type="button"
            onClick={() => setExportMenuOpen((open) => !open)}
            aria-label={t('dictionary.export')}
            title={t('dictionary.export')}
            aria-haspopup="menu"
            aria-expanded={exportMenuOpen}
            className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[6px] border border-border bg-transparent text-text-secondary hover:border-border-focus hover:text-text-primary"
          >
            <Download size={14} />
          </button>
          {exportMenuOpen && (
            <>
              <div className="fixed inset-0 z-30" onClick={closeExportMenu} />
              <div
                role="menu"
                className="absolute right-0 top-9 z-40 min-w-32 rounded-[8px] border border-border bg-bg-primary py-1 shadow-float"
              >
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => void exportContent('json')}
                  className="flex h-8 w-full items-center gap-2 bg-transparent px-3 text-left text-[12px] text-text-primary hover:bg-bg-secondary"
                >
                  <FileJson size={13} />
                  {t('dictionary.exportJson')}
                </button>
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => void exportContent('csv')}
                  className="flex h-8 w-full items-center gap-2 bg-transparent px-3 text-left text-[12px] text-text-primary hover:bg-bg-secondary"
                >
                  <FileSpreadsheet size={13} />
                  {t('dictionary.exportCsv')}
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      <div className="max-w-[240px]">
        <SegmentedControl
          options={[
            { value: 'words', label: t('dictionary.words') },
            { value: 'corrections', label: t('dictionary.corrections') },
          ]}
          value={activeSection}
          onChange={(value) => setActiveSection(value as 'words' | 'corrections')}
        />
      </div>

      {activeSection === 'words' && (
        <>
          <div className="grid grid-cols-1 gap-2 min-[840px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
            <input
              value={word}
              onChange={(event) => setWord(event.target.value)}
              placeholder={t('dictionary.word')}
              className="min-w-0 rounded-[8px] border border-border bg-bg-secondary px-3 py-2.5 text-[13px] text-text-primary outline-none focus:border-border-focus"
            />
            <input
              value={pronunciation}
              onChange={(event) => setPronunciation(event.target.value)}
              placeholder={t('dictionary.pronunciationOptional')}
              className="min-w-0 rounded-[8px] border border-border bg-bg-secondary px-3 py-2.5 text-[13px] text-text-primary outline-none focus:border-border-focus"
            />
            <button
              type="button"
              onClick={() => void handleAdd()}
              disabled={!word.trim()}
              className="flex items-center justify-center gap-1.5 rounded-[8px] border-none bg-accent px-4 py-2.5 text-[13px] text-white hover:bg-accent-hover disabled:opacity-40"
            >
              <Plus size={14} />
              {t('dictionary.add')}
            </button>
          </div>

          <div className="overflow-hidden rounded-[8px] border border-border">
            <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_64px] gap-2 bg-bg-secondary px-3 py-2.5 text-[11px] font-medium uppercase text-text-secondary">
              <span>{t('dictionary.word')}</span>
              <span>{t('dictionary.pronunciation')}</span>
              <span />
            </div>
            {filteredDictionary.length === 0 ? (
              <div className="px-3 py-8 text-center text-[13px] text-text-tertiary">
                {t('dictionary.noEntries')}
              </div>
            ) : (
              filteredDictionary.map((entry) =>
                editing?.kind === 'dictionary' && editing.id === entry.id ? (
                  <div
                    key={entry.id}
                    className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_64px] items-center gap-2 border-t border-border px-3 py-2"
                  >
                    <input
                      aria-label={t('dictionary.word')}
                      value={editing.word}
                      onChange={(event) => setEditing({ ...editing, word: event.target.value })}
                      className="min-w-0 rounded-[6px] border border-border bg-bg-secondary px-2 py-1.5 text-[12px] text-text-primary outline-none focus:border-border-focus"
                    />
                    <input
                      aria-label={t('dictionary.pronunciation')}
                      value={editing.pronunciation}
                      onChange={(event) =>
                        setEditing({ ...editing, pronunciation: event.target.value })
                      }
                      className="min-w-0 rounded-[6px] border border-border bg-bg-secondary px-2 py-1.5 text-[12px] text-text-primary outline-none focus:border-border-focus"
                    />
                    <div className="flex justify-end">
                      <button
                        type="button"
                        onClick={() => void saveEdit()}
                        aria-label={t('dictionary.saveEdit')}
                        title={t('dictionary.saveEdit')}
                        className="p-1.5 text-success"
                      >
                        <Check size={14} />
                      </button>
                      <button
                        type="button"
                        onClick={() => setEditing(null)}
                        aria-label={t('dictionary.cancelEdit')}
                        title={t('dictionary.cancelEdit')}
                        className="p-1.5 text-text-tertiary hover:text-text-primary"
                      >
                        <X size={14} />
                      </button>
                    </div>
                  </div>
                ) : (
                  <div
                    key={entry.id}
                    className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_64px] gap-2 border-t border-border px-3 py-2.5 text-[13px] hover:bg-bg-secondary/50"
                  >
                    <span className="min-w-0 truncate text-text-primary">{entry.word}</span>
                    <span className="min-w-0 truncate text-text-secondary">
                      {entry.pronunciation || '-'}
                    </span>
                    <div className="flex justify-end">
                      <button
                        type="button"
                        onClick={() =>
                          setEditing({
                            kind: 'dictionary',
                            id: entry.id,
                            word: entry.word,
                            pronunciation: entry.pronunciation ?? '',
                          })
                        }
                        aria-label={`${t('dictionary.editEntry')} ${entry.word}`}
                        title={t('dictionary.editEntry')}
                        className="p-1.5 text-text-tertiary hover:text-text-primary"
                      >
                        <Pencil size={13} />
                      </button>
                      <button
                        type="button"
                        onClick={() => void handleRemove(entry.id)}
                        aria-label={t('dictionary.removeEntry')}
                        title={t('dictionary.removeEntry')}
                        className="p-1.5 text-text-tertiary hover:text-error"
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </div>
                ),
              )
            )}
          </div>
        </>
      )}

      {activeSection === 'corrections' && (
        <div className="space-y-3">
          <div className="grid grid-cols-1 gap-2 min-[840px]:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
            <input
              value={pattern}
              onChange={(event) => setPattern(event.target.value)}
              placeholder={t('dictionary.wrongPhrase')}
              className="min-w-0 rounded-[8px] border border-border bg-bg-secondary px-3 py-2.5 text-[13px] text-text-primary outline-none focus:border-border-focus"
            />
            <input
              value={replacement}
              onChange={(event) => setReplacement(event.target.value)}
              placeholder={t('dictionary.correctPhrase')}
              className="min-w-0 rounded-[8px] border border-border bg-bg-secondary px-3 py-2.5 text-[13px] text-text-primary outline-none focus:border-border-focus"
            />
            <button
              type="button"
              onClick={() => void handleAddCorrection()}
              disabled={!pattern.trim() || !replacement.trim()}
              aria-label={t('dictionary.addCorrection')}
              className="flex items-center justify-center gap-1.5 rounded-[8px] border-none bg-accent px-4 py-2.5 text-[13px] text-white hover:bg-accent-hover disabled:opacity-40"
            >
              <Plus size={14} />
              {t('dictionary.add')}
            </button>
          </div>

          <div className="overflow-hidden rounded-[8px] border border-border">
            {filteredCorrections.length === 0 ? (
              <div className="px-3 py-8 text-center text-[13px] text-text-tertiary">
                {t('dictionary.noCorrections')}
              </div>
            ) : (
              filteredCorrections.map((rule) =>
                editing?.kind === 'correction' && editing.id === rule.id ? (
                  <div
                    key={rule.id}
                    className="grid grid-cols-[auto_minmax(0,1fr)_minmax(0,1fr)_64px] items-center gap-2 border-t first:border-t-0 border-border px-3 py-2"
                  >
                    <input type="checkbox" checked={editing.enabled} readOnly className="h-4 w-4" />
                    <input
                      aria-label={t('dictionary.wrongPhrase')}
                      value={editing.pattern}
                      onChange={(event) => setEditing({ ...editing, pattern: event.target.value })}
                      className="min-w-0 rounded-[6px] border border-border bg-bg-secondary px-2 py-1.5 text-[12px] text-text-primary outline-none focus:border-border-focus"
                    />
                    <input
                      aria-label={t('dictionary.correctPhrase')}
                      value={editing.replacement}
                      onChange={(event) =>
                        setEditing({ ...editing, replacement: event.target.value })
                      }
                      className="min-w-0 rounded-[6px] border border-border bg-bg-secondary px-2 py-1.5 text-[12px] text-text-primary outline-none focus:border-border-focus"
                    />
                    <div className="flex justify-end">
                      <button
                        type="button"
                        onClick={() => void saveEdit()}
                        aria-label={t('dictionary.saveEdit')}
                        title={t('dictionary.saveEdit')}
                        className="p-1.5 text-success"
                      >
                        <Check size={14} />
                      </button>
                      <button
                        type="button"
                        onClick={() => setEditing(null)}
                        aria-label={t('dictionary.cancelEdit')}
                        title={t('dictionary.cancelEdit')}
                        className="p-1.5 text-text-tertiary hover:text-text-primary"
                      >
                        <X size={14} />
                      </button>
                    </div>
                  </div>
                ) : (
                  <div
                    key={rule.id}
                    className="grid grid-cols-[auto_minmax(0,1fr)_64px] items-center gap-3 border-t first:border-t-0 border-border px-3 py-2.5 text-[13px] hover:bg-bg-secondary/50"
                  >
                    <input
                      type="checkbox"
                      checked={rule.enabled}
                      onChange={(event) =>
                        void handleToggleCorrection(rule.id, event.target.checked)
                      }
                      aria-label={t('dictionary.toggleCorrection')}
                      className="h-4 w-4 accent-accent"
                    />
                    <div className="min-w-0">
                      <p className="truncate text-text-primary">{rule.pattern}</p>
                      <p className="truncate text-[12px] text-text-secondary">{rule.replacement}</p>
                    </div>
                    <div className="flex justify-end">
                      <button
                        type="button"
                        onClick={() =>
                          setEditing({
                            kind: 'correction',
                            id: rule.id,
                            pattern: rule.pattern,
                            replacement: rule.replacement,
                            enabled: rule.enabled,
                          })
                        }
                        aria-label={`${t('dictionary.editCorrection')} ${rule.pattern}`}
                        title={t('dictionary.editCorrection')}
                        className="p-1.5 text-text-tertiary hover:text-text-primary"
                      >
                        <Pencil size={13} />
                      </button>
                      <button
                        type="button"
                        onClick={() => void handleRemoveCorrection(rule.id)}
                        aria-label={t('dictionary.removeCorrection')}
                        title={t('dictionary.removeCorrection')}
                        className="p-1.5 text-text-tertiary hover:text-error"
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </div>
                ),
              )
            )}
          </div>
        </div>
      )}

      {pendingImport && (
        <DictionaryImportDialog
          fileName={pendingImport.fileName}
          report={pendingImport.report}
          committing={committingImport}
          onCancel={closeImport}
          onConfirm={() => void confirmImport()}
        />
      )}
    </div>
  )
}
