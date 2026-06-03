import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { UpgradePage } from '../index'

const mockAuthState = {
  user: null,
  plan: 'free' as const,
  sttSecondsUsed: 0,
  sttSecondsLimit: 0,
  llmTokensUsed: 0,
  llmTokensLimit: 0,
}

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn(),
}))

vi.mock('../../../lib/api', () => ({
  createCheckout: vi.fn().mockResolvedValue({ url: 'https://checkout.example.test' }),
}))

vi.mock('../../../stores/authStore', () => ({
  useAuthStore: Object.assign(() => mockAuthState, {
    setState: vi.fn(),
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      (
        ({
          'upgrade.title': 'Upgrade to Pro',
          'upgrade.subtitle': '300-800ms voice recognition + AI rewriting. 99 languages.',
          'upgrade.currentPlan': `Current plan: ${values?.plan ?? ''}`,
          'upgrade.pro': 'Pro',
          'upgrade.free': 'Free',
          'upgrade.month': 'month',
          'upgrade.subscribeToPro': 'Subscribe to Pro',
          'upgrade.signInFirst': 'Sign in from the Account page first to subscribe.',
          'upgrade.benefits.title': 'Pro includes',
          'upgrade.benefits.stt': '10h cloud speech recognition every month',
          'upgrade.benefits.llm': '~5M AI tokens for polishing and rewriting',
          'upgrade.benefits.noApiKey': 'No API keys required in cloud mode',
          'upgrade.benefits.backupScenes': 'Cloud backup and scene templates included',
        }) as Record<string, string>
      )[key] ?? key,
  }),
}))

beforeEach(() => {
  Object.assign(mockAuthState, {
    user: null,
    plan: 'free' as const,
    sttSecondsUsed: 0,
    sttSecondsLimit: 0,
    llmTokensUsed: 0,
    llmTokensLimit: 0,
  })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('UpgradePage', () => {
  it('clearly shows concrete Pro entitlements before subscribing', () => {
    render(<UpgradePage />)

    expect(screen.getByRole('heading', { name: 'Pro includes' })).toBeInTheDocument()
    expect(screen.getByText('10h cloud speech recognition every month')).toBeInTheDocument()
    expect(screen.getByText('~5M AI tokens for polishing and rewriting')).toBeInTheDocument()
    expect(screen.getByText('No API keys required in cloud mode')).toBeInTheDocument()
  })
})
