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
        'dictionary.pronunciation': 'Pronunciation',
        'dictionary.pronunciationOptional': 'Pronunciation optional',
        'dictionary.add': 'Add',
        'dictionary.addCorrection': 'Add correction',
        'dictionary.noEntries': 'No words yet',
        'dictionary.corrections': 'Corrections',
        'dictionary.wrongPhrase': 'Wrong phrase',
        'dictionary.correctPhrase': 'Correct phrase',
        'dictionary.noCorrections': 'No corrections yet',
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
  })

  afterEach(() => {
    cleanup()
  })

  it('renders a small corrections area below dictionary words', () => {
    render(<DictionaryPane />)

    expect(screen.getByText('Corrections')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Wrong phrase')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Correct phrase')).toBeInTheDocument()
    expect(screen.getByText('No corrections yet')).toBeInTheDocument()
  })

  it('adds a correction rule and refreshes correction rules', async () => {
    vi.mocked(tauri.getCorrectionRules).mockResolvedValueOnce([
      { id: 1, pattern: '拓肯', replacement: 'Token', enabled: true },
    ])

    render(<DictionaryPane />)

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
})
