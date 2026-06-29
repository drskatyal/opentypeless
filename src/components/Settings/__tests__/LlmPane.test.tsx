import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { LlmPane } from '../LlmPane'
import * as tauri from '../../../lib/tauri'

// Mock Tauri
vi.mock('../../../lib/tauri')

// Mock i18n
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, params?: any) => {
      const translations: Record<string, string> = {
        'settings.provider': 'Provider',
        'settings.apiKey': 'API Key',
        'settings.model': 'Model',
        'settings.baseUrl': 'Base URL',
        'settings.test': 'Test',
        'settings.enterApiKey': 'Enter API Key',
        'settings.connectionSuccess': 'Connection successful',
        'settings.connectionFailed': 'Connection failed',
        'settings.storedLocally': 'Stored locally',
        'settings.fetchModels': 'Fetch models',
        'settings.modelsAvailable': `${params?.count || 0} models available`,
        'settings.llmModelPlaceholder': 'e.g. gpt-4o-mini',
        'settings.enableAiPolish': 'Enable AI Polish',
        'settings.advancedPolishSettings': 'Advanced polish settings',
        'settings.advancedPolishSettingsDesc': 'Optional writing rules',
        'settings.customPolishInstructions': 'Custom polish instructions',
        'settings.customPolishInstructionsPlaceholder': 'Example prompt',
        'settings.customPolishInstructionsCount': `${params?.count || 0} / 2000 characters`,
        'settings.translationMode': 'Translation Mode',
        'settings.selectedTextContext': 'Selected Text Context',
        'settings.selectedTextContextDesc': 'Include selected text as context',
        'settings.targetLanguage': 'Target Language',
        'settings.cloudLlmPro': 'Cloud LLM (Pro)',
        'settings.llmSignInHint': 'Sign in to use cloud LLM',
        'settings.llmUpgradeHint': 'Upgrade to Pro to use cloud LLM',
        'settings.llmProActive': 'Cloud LLM active',
        'settings.askAnything': 'Ask Anything',
        'settings.askAnythingDesc': 'Voice question, one-shot answer. No chat history.',
        'ask.ready': 'Ready to ask',
        'ask.listening': 'Listening',
        'ask.thinking': 'Thinking',
        'ask.voiceQuestion': 'Voice question',
        'ask.voiceQuestionDesc': 'Speak your question. Stop recording to answer.',
        'ask.transcriptLabel': 'Question transcript',
        'ask.answerLabel': 'Answer',
        'ask.manualFallback': 'Type instead',
        'ask.placeholder': 'Type a question, or use the capsule above.',
        'ask.recordQuestion': 'Record question',
        'ask.stopAndAsk': 'Stop and ask',
        'ask.send': 'Ask',
        'providers.llm.doubao': 'Doubao (Volcengine)',
      }
      return translations[key] || key
    },
  }),
}))

// Mock stores - must be done before importing the component
const mockAppStore = {
  config: {
    llm_provider: 'openai' as string,
    llm_api_key: '',
    llm_base_url: 'https://api.openai.com/v1',
    llm_model: 'gpt-4o-mini',
    polish_enabled: true,
    polish_custom_prompt: '',
    polish_chinese_script: 'preserve',
    translate_enabled: false,
    selected_text_enabled: false,
    target_lang: 'en',
  },
  updateConfig: vi.fn(),
  llmTestStatus: 'idle' as 'idle' | 'testing' | 'success' | 'error',
  setLlmTestStatus: vi.fn(),
  llmLatencyMs: null as number | null,
  setLlmLatencyMs: vi.fn(),
  llmModels: [] as string[],
  setLlmModels: vi.fn(),
}

const mockAuthStore = {
  user: null as any,
  plan: null as any,
  source: 'free',
  cloudWordsLimit: 0,
  licenseStatus: null as any,
}

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => {
    if (typeof selector === 'function') {
      return selector(mockAppStore)
    }
    return mockAppStore
  },
}))

vi.mock('../../../stores/authStore', () => ({
  hasManagedCloudAccess: (state: typeof mockAuthStore) =>
    state.licenseStatus !== 'refunded' &&
    state.licenseStatus !== 'deactivated' &&
    ((state.source === 'creem' && state.cloudWordsLimit > 0) ||
      (state.source === 'appsumo' &&
        state.cloudWordsLimit > 0 &&
        state.licenseStatus === 'active') ||
      state.plan === 'pro'),
  useAuthStore: (selector: any) => {
    if (typeof selector === 'function') {
      return selector(mockAuthStore)
    }
    return mockAuthStore
  },
}))

describe('LlmPane', () => {
  beforeEach(() => {
    // Reset mock store state
    mockAppStore.config = {
      llm_provider: 'openai',
      llm_api_key: '',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-4o-mini',
      polish_enabled: true,
      polish_custom_prompt: '',
      polish_chinese_script: 'preserve',
      translate_enabled: false,
      selected_text_enabled: false,
      target_lang: 'en',
    }
    mockAppStore.llmTestStatus = 'idle'
    mockAppStore.llmLatencyMs = null
    mockAppStore.llmModels = []
    mockAuthStore.user = null
    mockAuthStore.plan = null
    mockAuthStore.source = 'free'
    mockAuthStore.cloudWordsLimit = 0
    mockAuthStore.licenseStatus = null
  })

  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  describe('Provider selection', () => {
    it('renders provider dropdown with current value', () => {
      render(<LlmPane />)
      const selects = screen.getAllByRole('combobox')
      const providerSelect = selects[0] // First select is provider
      expect(providerSelect).toHaveValue('openai')
    })

    it('updates config and resets state when provider changes', () => {
      render(<LlmPane />)
      const selects = screen.getAllByRole('combobox')
      const providerSelect = selects[0]

      fireEvent.change(providerSelect, { target: { value: 'anthropic' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalled()
      expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
      expect(mockAppStore.setLlmModels).toHaveBeenCalledWith([])
    })

    it('applies Doubao defaults when provider changes to Doubao', () => {
      render(<LlmPane />)
      const selects = screen.getAllByRole('combobox')
      const providerSelect = selects[0]

      fireEvent.change(providerSelect, { target: { value: 'doubao' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({
        llm_provider: 'doubao',
        llm_base_url: 'https://ark.cn-beijing.volces.com/api/v3',
        llm_model: 'doubao-seed-1-6-flash-250615',
      })
      expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
      expect(mockAppStore.setLlmModels).toHaveBeenCalledWith([])
    })
  })

  describe('Cloud provider UI', () => {
    it('renders Ask Anything as a voice-first one-shot panel', () => {
      render(<LlmPane />)

      expect(screen.getByText('Ask Anything')).toBeInTheDocument()
      expect(
        screen.getByText('Voice question, one-shot answer. No chat history.'),
      ).toBeInTheDocument()
      expect(screen.getByText('Voice question')).toBeInTheDocument()
      expect(screen.getByText('Ready to ask')).toBeInTheDocument()
    })

    it('shows cloud info when provider is cloud and user not signed in', () => {
      mockAppStore.config.llm_provider = 'cloud'
      render(<LlmPane />)
      expect(screen.getByText('Sign in to use cloud LLM')).toBeInTheDocument()
    })

    it('shows upgrade hint when user is signed in but not pro', () => {
      mockAppStore.config.llm_provider = 'cloud'
      mockAuthStore.user = { id: '1', email: 'test@example.com' }
      mockAuthStore.plan = 'free'

      render(<LlmPane />)
      expect(screen.getByText('Upgrade to Pro to use cloud LLM')).toBeInTheDocument()
    })

    it('shows active status when user is pro', () => {
      mockAppStore.config.llm_provider = 'cloud'
      mockAuthStore.user = { id: '1', email: 'test@example.com' }
      mockAuthStore.plan = 'pro'

      render(<LlmPane />)
      expect(screen.getByText('Cloud LLM active')).toBeInTheDocument()
    })
  })

  describe('API Key input', () => {
    it('renders API key input with current value', () => {
      mockAppStore.config.llm_api_key = 'sk-test123'
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('Enter API Key') as HTMLInputElement
      expect(input.value).toBe('sk-test123')
      expect(input.type).toBe('password')
    })

    it('updates config and resets test state when API key changes', () => {
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('Enter API Key')

      fireEvent.change(input, { target: { value: 'sk-new-key' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ llm_api_key: 'sk-new-key' })
      expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
    })
  })

  describe('Test button and latency display', () => {
    it('test button is disabled when API key is empty', () => {
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('test button is enabled when API key is present', () => {
      mockAppStore.config.llm_api_key = 'sk-test123'
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).not.toBeDisabled()
    })

    it('shows loading state during test', () => {
      mockAppStore.config.llm_api_key = 'sk-test123'
      mockAppStore.llmTestStatus = 'testing'
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('calls benchLlmConnection on test button click', async () => {
      const mockBenchLlm = vi.mocked(tauri.benchLlmConnection)
      mockBenchLlm.mockResolvedValue(187)

      mockAppStore.config.llm_api_key = 'sk-test123'
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })

      fireEvent.click(button)

      await waitFor(() => {
        expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('testing')
        expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
      })

      await waitFor(() => {
        expect(mockBenchLlm).toHaveBeenCalledWith(
          'sk-test123',
          'openai',
          'https://api.openai.com/v1',
          'gpt-4o-mini',
        )
      })
    })

    it('displays latency in milliseconds when test succeeds', () => {
      mockAppStore.config.llm_api_key = 'sk-test123'
      mockAppStore.llmTestStatus = 'success'
      mockAppStore.llmLatencyMs = 187

      render(<LlmPane />)
      expect(screen.getByText('187ms')).toBeInTheDocument()
    })

    it('displays generic success message when latency is null', () => {
      mockAppStore.config.llm_api_key = 'sk-test123'
      mockAppStore.llmTestStatus = 'success'
      mockAppStore.llmLatencyMs = null

      render(<LlmPane />)
      expect(screen.getByText('Connection successful')).toBeInTheDocument()
    })

    it('shows error state UI', () => {
      mockAppStore.config.llm_api_key = 'sk-test123'
      mockAppStore.llmTestStatus = 'error'

      render(<LlmPane />)
      expect(screen.getByText('Connection failed')).toBeInTheDocument()
    })

    it('shows backend error details when test fails', async () => {
      const mockBenchLlm = vi.mocked(tauri.benchLlmConnection)
      mockBenchLlm.mockRejectedValue('HTTP 429 Too Many Requests')

      mockAppStore.config.llm_api_key = 'sk-test123'
      render(<LlmPane />)

      fireEvent.click(screen.getByRole('button', { name: /test/i }))

      expect(await screen.findByText('HTTP 429 Too Many Requests')).toBeInTheDocument()
      expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('error')
    })
  })

  describe('AI polish behavior settings', () => {
    it('keeps custom instruction controls inside advanced settings', () => {
      render(<LlmPane />)

      expect(screen.getByText('Advanced polish settings')).toBeInTheDocument()
      expect(screen.queryByText('Chinese output')).not.toBeInTheDocument()
      expect(screen.queryByText('Custom polish instructions')).not.toBeInTheDocument()

      fireEvent.click(screen.getByRole('button', { name: /advanced polish settings/i }))

      expect(screen.getByText('Custom polish instructions')).toBeInTheDocument()
      expect(screen.queryByText('Chinese output')).not.toBeInTheDocument()
    })

    it('updates custom polish instructions from advanced settings', () => {
      render(<LlmPane />)

      fireEvent.click(screen.getByRole('button', { name: /advanced polish settings/i }))
      const textarea = screen.getByPlaceholderText('Example prompt')
      fireEvent.change(textarea, { target: { value: 'Keep a concise professional tone.' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({
        polish_custom_prompt: 'Keep a concise professional tone.',
      })
    })

    it('opens advanced settings automatically when custom instructions exist', () => {
      mockAppStore.config.polish_custom_prompt = 'Keep it concise.'

      render(<LlmPane />)

      expect(screen.getByText('Custom polish instructions')).toBeInTheDocument()
      expect(screen.queryByText('Chinese output')).not.toBeInTheDocument()
    })
  })

  describe('Model input', () => {
    it('updates config and resets latency when model changes', () => {
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('e.g. gpt-4o-mini')

      fireEvent.change(input, { target: { value: 'gpt-4o' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ llm_model: 'gpt-4o' })
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
    })

    it('displays available models count', () => {
      mockAppStore.llmModels = ['gpt-4o', 'gpt-4o-mini', 'gpt-3.5-turbo']

      render(<LlmPane />)
      expect(screen.getByText('3 models available')).toBeInTheDocument()
    })
  })

  describe('Base URL input', () => {
    it('updates config when base URL changes', () => {
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('https://api.openai.com/v1')

      fireEvent.change(input, { target: { value: 'https://custom.api.com/v1' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({
        llm_base_url: 'https://custom.api.com/v1',
      })
    })
  })

  describe('Feature toggles', () => {
    it('shows target language selector when translation is enabled', () => {
      mockAppStore.config.translate_enabled = true

      render(<LlmPane />)
      expect(screen.getByText('Target Language')).toBeInTheDocument()
    })
  })
})
