import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { SttSetupStep } from '../SttSetupStep'

const mockStore = {
  config: {
    stt_provider: 'gemini',
    stt_api_key: '',
  },
  updateConfig: vi.fn(),
  sttTestStatus: 'idle',
  setSttTestStatus: vi.fn(),
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'onboarding.stt.apiKeyLabel': 'Gemini API key',
        'onboarding.stt.apiKeyPlaceholder': 'API key',
        'onboarding.stt.testButton': 'Test',
        'onboarding.stt.connectionOk': 'OK',
        'onboarding.stt.connectionFail': 'Failed',
        'onboarding.stt.geminiKeyHint': 'Get a key',
      })[key] ?? key,
  }),
}))

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => selector(mockStore),
}))

vi.mock('../../../lib/tauri', () => ({
  testSttConnection: vi.fn().mockResolvedValue(true),
}))

beforeEach(() => {
  mockStore.config = {
    stt_provider: 'gemini',
    stt_api_key: '',
  }
  mockStore.updateConfig = vi.fn()
  mockStore.sttTestStatus = 'idle'
  mockStore.setSttTestStatus = vi.fn()
})

afterEach(() => cleanup())

describe('SttSetupStep', () => {
  it('collects a Gemini API key without a provider selector', () => {
    render(<SttSetupStep />)

    // Gemini-only: no provider dropdown, just the API key field.
    expect(screen.queryByRole('combobox')).toBeNull()
    expect(screen.getByPlaceholderText('API key')).toBeInTheDocument()
  })

  it('forces the Gemini provider when a stale provider is configured', () => {
    mockStore.config = { ...mockStore.config, stt_provider: 'deepgram' }

    render(<SttSetupStep />)

    expect(mockStore.updateConfig).toHaveBeenCalledWith({ stt_provider: 'gemini' })
  })

  it('tests the connection with the entered key against Gemini', async () => {
    const tauri = await import('../../../lib/tauri')
    mockStore.config = { ...mockStore.config, stt_api_key: 'gemini-secret' }

    render(<SttSetupStep />)
    fireEvent.click(screen.getByRole('button', { name: 'Test' }))

    await waitFor(() => {
      expect(tauri.testSttConnection).toHaveBeenCalledWith('gemini-secret', 'gemini')
    })
  })
})
