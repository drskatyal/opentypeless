import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { LlmSetupStep } from '../LlmSetupStep'
import * as tauri from '../../../lib/tauri'

const mockStore = {
  config: {
    llm_provider: 'gemini',
    llm_api_key: '',
    llm_base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
    llm_model: 'gemini-3.5-flash-lite',
  },
  updateConfig: vi.fn(),
  llmTestStatus: 'idle',
  setLlmTestStatus: vi.fn(),
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'onboarding.llm.apiKeyLabel': 'Gemini API key',
        'onboarding.llm.apiKeyPlaceholder': 'API key',
        'onboarding.llm.testButton': 'Test',
        'onboarding.llm.connectionOk': 'OK',
        'onboarding.llm.connectionFail': 'Failed',
        'onboarding.llm.geminiKeyHint': 'Reuse your key',
      })[key] ?? key,
  }),
}))

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => selector(mockStore),
}))

vi.mock('../../../lib/tauri')

beforeEach(() => {
  mockStore.config = {
    llm_provider: 'gemini',
    llm_api_key: '',
    llm_base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
    llm_model: 'gemini-3.5-flash-lite',
  }
  mockStore.updateConfig = vi.fn()
  mockStore.llmTestStatus = 'idle'
  mockStore.setLlmTestStatus = vi.fn()
  vi.clearAllMocks()
  vi.mocked(tauri.testLlmConnection).mockResolvedValue(true)
})

afterEach(cleanup)

describe('LlmSetupStep', () => {
  it('collects a Gemini API key without a provider or model selector', () => {
    render(<LlmSetupStep />)

    // Gemini-only: no provider dropdown, no model/base-url fields.
    expect(screen.queryByRole('combobox')).toBeNull()
    expect(screen.getByPlaceholderText('API key')).toBeInTheDocument()
  })

  it('forces the Gemini provider and defaults when a stale provider is configured', () => {
    mockStore.config = { ...mockStore.config, llm_provider: 'ollama' }

    render(<LlmSetupStep />)

    expect(mockStore.updateConfig).toHaveBeenCalledWith({
      llm_provider: 'gemini',
      llm_base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
      llm_model: 'gemini-3.5-flash-lite',
    })
  })

  it('tests the connection with the entered key against Gemini', async () => {
    mockStore.config = { ...mockStore.config, llm_api_key: 'gemini-secret' }

    render(<LlmSetupStep />)
    fireEvent.click(screen.getByRole('button', { name: 'Test' }))

    await waitFor(() =>
      expect(tauri.testLlmConnection).toHaveBeenCalledWith(
        'gemini-secret',
        'gemini',
        'https://generativelanguage.googleapis.com/v1beta/openai',
        'gemini-3.5-flash-lite',
      ),
    )
  })
})
