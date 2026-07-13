import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DictionaryPane } from '../DictionaryPane'
import * as tauri from '../../../lib/tauri'

vi.mock('../../../lib/tauri')

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const translations: Record<string, string> = {
        'dictionary.word': 'Word',
        'dictionary.words': 'Words',
        'dictionary.pronunciation': 'Pronunciation',
        'dictionary.pronunciationOptional': 'Pronunciation optional',
        'dictionary.add': 'Add',
        'dictionary.addCorrection': 'Add correction',
        'dictionary.noEntries': 'No words yet',
        'dictionary.corrections': 'Corrections',
        'dictionary.wrongPhrase': 'Wrong phrase',
        'dictionary.correctPhrase': 'Correct phrase',
        'dictionary.noCorrections': 'No corrections yet',
        'dictionary.searchPlaceholder': 'Search dictionary',
        'dictionary.import': 'Import dictionary',
        'dictionary.importFile': 'Import dictionary file',
        'dictionary.export': 'Export dictionary',
        'dictionary.exportJson': 'Export JSON',
        'dictionary.exportCsv': 'Export CSV',
        'dictionary.editEntry': 'Edit entry',
        'dictionary.editCorrection': 'Edit correction',
        'dictionary.saveEdit': 'Save edit',
        'dictionary.cancelEdit': 'Cancel edit',
        'dictionary.importTitle': 'Import dictionary',
        'dictionary.importAccepted': 'Accepted',
        'dictionary.importDuplicates': 'Duplicates',
        'dictionary.importInvalid': 'Invalid',
        'dictionary.confirmImport': 'Import',
        'common.cancel': 'Cancel',
      }
      return translations[key] || key
    },
  }),
}))

const mockAppStore = {
  dictionary: [] as Array<{ id: number; word: string; pronunciation: string | null }>,
  setDictionary: vi.fn(),
  correctionRules: [] as Array<{
    id: number
    pattern: string
    replacement: string
    enabled: boolean
  }>,
  setCorrectionRules: vi.fn(),
}

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => selector(mockAppStore),
}))

describe('DictionaryPane', () => {
  beforeEach(() => {
    mockAppStore.dictionary = []
    mockAppStore.correctionRules = []
    vi.clearAllMocks()
    vi.mocked(tauri.getDictionary).mockResolvedValue([])
    vi.mocked(tauri.getCorrectionRules).mockResolvedValue([])
    vi.mocked(tauri.addDictionaryEntry).mockResolvedValue(undefined)
    vi.mocked(tauri.removeDictionaryEntry).mockResolvedValue(undefined)
    vi.mocked(tauri.addCorrectionRule).mockResolvedValue(undefined)
    vi.mocked(tauri.removeCorrectionRule).mockResolvedValue(undefined)
    vi.mocked(tauri.setCorrectionRuleEnabled).mockResolvedValue(undefined)
    vi.mocked(tauri.updateDictionaryEntry).mockResolvedValue(undefined)
    vi.mocked(tauri.updateCorrectionRule).mockResolvedValue(undefined)
    vi.mocked(tauri.previewDictionaryImport).mockResolvedValue({
      accepted: 0,
      skippedDuplicates: 0,
      skippedInvalid: 0,
      errors: [],
    })
    vi.mocked(tauri.commitDictionaryImport).mockResolvedValue({
      accepted: 0,
      skippedDuplicates: 0,
      skippedInvalid: 0,
      errors: [],
    })
    vi.mocked(tauri.exportDictionaryJson).mockResolvedValue('{}')
    vi.mocked(tauri.exportDictionaryCsv).mockResolvedValue('type,word\n')
  })

  afterEach(() => {
    cleanup()
  })

  it('shows words and corrections as separate compact sections', () => {
    render(<DictionaryPane />)

    expect(screen.getByRole('button', { name: 'Words' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Corrections' })).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Pronunciation optional')).toBeInTheDocument()
    expect(screen.queryByPlaceholderText('Wrong phrase')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Corrections' }))

    expect(screen.getByPlaceholderText('Wrong phrase')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Correct phrase')).toBeInTheDocument()
    expect(screen.getByText('No corrections yet')).toBeInTheDocument()
    expect(screen.queryByPlaceholderText('Pronunciation optional')).toBeNull()
  })

  it('preserves unfinished drafts when switching sections', () => {
    render(<DictionaryPane />)

    fireEvent.change(screen.getByPlaceholderText('Word'), { target: { value: 'TalkMore' } })
    fireEvent.change(screen.getByPlaceholderText('Pronunciation optional'), {
      target: { value: 'talk more' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Corrections' }))
    fireEvent.change(screen.getByPlaceholderText('Wrong phrase'), {
      target: { value: 'talk more app' },
    })
    fireEvent.change(screen.getByPlaceholderText('Correct phrase'), {
      target: { value: 'TalkMore' },
    })

    fireEvent.click(screen.getByRole('button', { name: 'Words' }))
    expect(screen.getByPlaceholderText('Word')).toHaveValue('TalkMore')
    expect(screen.getByPlaceholderText('Pronunciation optional')).toHaveValue('talk more')

    fireEvent.click(screen.getByRole('button', { name: 'Corrections' }))
    expect(screen.getByPlaceholderText('Wrong phrase')).toHaveValue('talk more app')
    expect(screen.getByPlaceholderText('Correct phrase')).toHaveValue('TalkMore')
  })

  it('adds a correction rule and refreshes correction rules', async () => {
    vi.mocked(tauri.getCorrectionRules).mockResolvedValueOnce([
      { id: 1, pattern: '拓肯', replacement: 'Token', enabled: true },
    ])

    render(<DictionaryPane />)
    fireEvent.click(screen.getByRole('button', { name: 'Corrections' }))

    fireEvent.change(screen.getByPlaceholderText('Wrong phrase'), {
      target: { value: ' 拓肯 ' },
    })
    fireEvent.change(screen.getByPlaceholderText('Correct phrase'), {
      target: { value: ' Token ' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Add correction' }))

    await waitFor(() => {
      expect(tauri.addCorrectionRule).toHaveBeenCalledWith('拓肯', 'Token')
      expect(mockAppStore.setCorrectionRules).toHaveBeenCalledWith([
        { id: 1, pattern: '拓肯', replacement: 'Token', enabled: true },
      ])
    })
  })

  it('searches words, pronunciations, wrong phrases, and replacements locally', () => {
    mockAppStore.dictionary = [
      { id: 1, word: 'OpenTypeless', pronunciation: 'open typeless' },
      { id: 2, word: 'MeloLab', pronunciation: 'mee-lo' },
    ]
    mockAppStore.correctionRules = [
      { id: 3, pattern: 'open type less', replacement: 'OpenTypeless', enabled: true },
    ]
    render(<DictionaryPane />)

    fireEvent.change(screen.getByPlaceholderText('Search dictionary'), {
      target: { value: 'mee-lo' },
    })
    expect(screen.getByText('MeloLab')).toBeInTheDocument()
    expect(screen.queryByText('OpenTypeless')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Corrections' }))
    fireEvent.change(screen.getByPlaceholderText('Search dictionary'), {
      target: { value: 'open type less' },
    })
    expect(screen.getByText('open type less')).toBeInTheDocument()
  })

  it('edits dictionary and correction rows inline', async () => {
    mockAppStore.dictionary = [{ id: 1, word: 'Token', pronunciation: null }]
    mockAppStore.correctionRules = [
      { id: 2, pattern: 'open type less', replacement: 'OpenTypeless', enabled: true },
    ]
    render(<DictionaryPane />)

    fireEvent.click(screen.getByRole('button', { name: 'Edit entry Token' }))
    fireEvent.change(screen.getByLabelText('Word'), { target: { value: 'TalkMore' } })
    fireEvent.change(screen.getByLabelText('Pronunciation'), {
      target: { value: 'talk more' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save edit' }))

    await waitFor(() =>
      expect(tauri.updateDictionaryEntry).toHaveBeenCalledWith(1, 'TalkMore', 'talk more'),
    )

    fireEvent.click(screen.getByRole('button', { name: 'Corrections' }))
    fireEvent.click(screen.getByRole('button', { name: 'Edit correction open type less' }))
    fireEvent.change(screen.getByLabelText('Wrong phrase'), { target: { value: 'token' } })
    fireEvent.change(screen.getByLabelText('Correct phrase'), { target: { value: 'Token' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save edit' }))

    await waitFor(() =>
      expect(tauri.updateCorrectionRule).toHaveBeenCalledWith(2, 'token', 'Token', true),
    )
  })

  it('previews and commits the same import bytes before refreshing both lists', async () => {
    vi.mocked(tauri.previewDictionaryImport).mockResolvedValue({
      accepted: 2,
      skippedDuplicates: 1,
      skippedInvalid: 1,
      errors: [{ row: 4, code: 'dictionary_word_too_long' }],
    })
    vi.mocked(tauri.commitDictionaryImport).mockResolvedValue({
      accepted: 2,
      skippedDuplicates: 1,
      skippedInvalid: 1,
      errors: [{ row: 4, code: 'dictionary_word_too_long' }],
    })
    const file = new File(['OpenTypeless'], 'terms.txt', { type: 'text/plain' })
    const bytes = new TextEncoder().encode('OpenTypeless')
    Object.defineProperty(file, 'arrayBuffer', {
      value: vi.fn().mockResolvedValue(bytes.buffer),
    })
    render(<DictionaryPane />)

    fireEvent.change(screen.getByLabelText('Import dictionary file'), {
      target: { files: [file] },
    })

    expect(await screen.findByRole('dialog', { name: 'Import dictionary' })).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
    expect(screen.getAllByText('1')).toHaveLength(2)
    fireEvent.click(screen.getByRole('button', { name: 'Import' }))

    await waitFor(() => {
      expect(tauri.previewDictionaryImport).toHaveBeenCalledWith(Array.from(bytes), 'txt')
      expect(tauri.commitDictionaryImport).toHaveBeenCalledWith(Array.from(bytes), 'txt')
      expect(tauri.getDictionary).toHaveBeenCalled()
      expect(tauri.getCorrectionRules).toHaveBeenCalled()
    })
  })

  it('exports JSON and CSV through the existing compact menu', async () => {
    const createObjectURL = vi.fn().mockReturnValue('blob:test')
    const revokeObjectURL = vi.fn()
    vi.stubGlobal('URL', { createObjectURL, revokeObjectURL })
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    render(<DictionaryPane />)

    fireEvent.click(screen.getByRole('button', { name: 'Export dictionary' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Export JSON' }))
    await waitFor(() => expect(tauri.exportDictionaryJson).toHaveBeenCalled())

    fireEvent.click(screen.getByRole('button', { name: 'Export dictionary' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Export CSV' }))
    await waitFor(() => expect(tauri.exportDictionaryCsv).toHaveBeenCalled())
    expect(createObjectURL).toHaveBeenCalledTimes(2)
    expect(click).toHaveBeenCalledTimes(2)

    click.mockRestore()
    vi.unstubAllGlobals()
  })
})
