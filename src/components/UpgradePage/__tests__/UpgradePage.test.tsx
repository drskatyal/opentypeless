import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { UpgradePage } from '../index'

type MockPlan =
  | 'free'
  | 'pro'
  | 'lifetime_starter'
  | 'appsumo_tier1'
  | 'appsumo_tier2'
  | 'appsumo_tier3'
type MockSource = 'free' | 'creem' | 'lifetime' | 'appsumo'
type MockLicenseStatus = 'pending' | 'active' | 'refunded' | 'deactivated' | null

const mockAuthState = {
  user: null,
  plan: 'free' as MockPlan,
  source: 'free' as MockSource,
  displayName: 'Free',
  licenseStatus: null as MockLicenseStatus,
  cloudWordsUsed: 0,
  cloudWordsLimit: 0,
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
  hasManagedCloudAccess: (state: typeof mockAuthState) =>
    state.licenseStatus !== 'refunded' &&
    state.licenseStatus !== 'deactivated' &&
    ((state.source === 'creem' && state.cloudWordsLimit > 0) ||
      (state.source === 'lifetime' && state.cloudWordsLimit > 0) ||
      (state.source === 'appsumo' &&
        state.cloudWordsLimit > 0 &&
        state.licenseStatus === 'active') ||
      state.plan === 'pro' ||
      state.plan === 'lifetime_starter'),
  useAuthStore: Object.assign(
    (selector: any) => {
      if (typeof selector === 'function') {
        return selector(mockAuthState)
      }
      return mockAuthState
    },
    {
      setState: vi.fn(),
    },
  ),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      (
        ({
          'upgrade.title': 'Upgrade',
          'upgrade.subtitle':
            'Fast voice recognition + AI rewriting with cloud words/month. 99 languages.',
          'upgrade.currentPlan': `Current plan: ${values?.plan ?? ''}`,
          'upgrade.pro': 'Pro Monthly',
          'upgrade.lifetime': 'Lifetime Starter',
          'upgrade.lifetimeBadge': 'Best value',
          'upgrade.lifetimeSave': 'Save after 18 months',
          'upgrade.lifetimeUpgradeSave': 'Includes your current monthly credit',
          'upgrade.free': 'Free',
          'upgrade.month': 'month',
          'upgrade.oneTime': 'one-time',
          'upgrade.subscribeToPro': 'Subscribe to Pro',
          'upgrade.buyLifetime': 'Buy lifetime',
          'upgrade.signInFirst': 'Sign in from the Account page first to subscribe.',
          'upgrade.benefits.title': 'What you get',
          'upgrade.benefits.cloudWords': '100,000 cloud words/month for voice and AI',
          'upgrade.benefits.noApiKey': 'No API keys required in cloud mode',
          'upgrade.benefits.backupScenes': 'Cloud backup and Pro scene packs',
          'upgrade.monthlyActive': 'Pro is active.',
        }) as Record<string, string>
      )[key] ?? key,
  }),
}))

beforeEach(() => {
  Object.assign(mockAuthState, {
    user: null,
    plan: 'free' as MockPlan,
    source: 'free' as MockSource,
    displayName: 'Free',
    licenseStatus: null as MockLicenseStatus,
    cloudWordsUsed: 0,
    cloudWordsLimit: 0,
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
  it('shows the monthly plan before subscribing', () => {
    render(<UpgradePage />)

    expect(screen.getByText('Pro Monthly')).toBeInTheDocument()
    expect(screen.getByText('$4.99')).toBeInTheDocument()
    expect(screen.queryByText('Lifetime Starter')).not.toBeInTheDocument()
    expect(screen.queryByText('$89.99')).not.toBeInTheDocument()
  })

  it('keeps the post-plan benefits short and focused', () => {
    render(<UpgradePage />)

    expect(screen.getByRole('heading', { name: 'What you get' })).toBeInTheDocument()
    expect(screen.getByText('100,000 cloud words/month for voice and AI')).toBeInTheDocument()
    expect(screen.getByText('No API keys required in cloud mode')).toBeInTheDocument()
    expect(screen.getByText('Cloud backup and Pro scene packs')).toBeInTheDocument()
    expect(screen.queryByText('upgrade.features.sttTitle')).not.toBeInTheDocument()
    expect(screen.queryByText('upgrade.features.llmTitle')).not.toBeInTheDocument()
    expect(screen.queryByText('upgrade.features.backupTitle')).not.toBeInTheDocument()
  })

  it('starts monthly checkout with the monthly product', async () => {
    const { createCheckout } = await import('../../../lib/api')
    Object.assign(mockAuthState, {
      user: { id: 'user-1', email: 'user@example.com', name: null },
    })

    render(<UpgradePage />)
    fireEvent.click(screen.getByRole('button', { name: 'Subscribe to Pro' }))

    await waitFor(() => {
      expect(createCheckout).toHaveBeenCalledWith('desktop', 'pro_monthly')
    })
  })

  it('does not show checkout actions for active monthly users while lifetime is disabled', () => {
    Object.assign(mockAuthState, {
      user: { id: 'user-1', email: 'user@example.com', name: null },
      plan: 'pro' as MockPlan,
      source: 'creem' as MockSource,
      displayName: 'Pro',
      cloudWordsLimit: 100000,
    })

    render(<UpgradePage />)
    expect(screen.queryByText('Pro Monthly')).not.toBeInTheDocument()
    expect(screen.queryByText('Buy lifetime')).not.toBeInTheDocument()
    expect(screen.getByText('Pro is active.')).toBeInTheDocument()
  })
})
