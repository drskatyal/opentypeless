import { describe, it, expect, vi, beforeEach } from 'vitest'
import { hasManagedCloudAccess, useAuthStore } from '../authStore'

// Mock external dependencies
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('../../lib/auth-client', () => ({
  authClient: {
    getSession: vi.fn(),
    signIn: { email: vi.fn() },
    signUp: { email: vi.fn() },
    sendVerificationEmail: vi.fn(),
    signOut: vi.fn(),
  },
}))

vi.mock('../../lib/api', () => ({
  getSubscriptionStatus: vi.fn(),
}))

vi.mock('../../components/Toast', () => ({
  toast: vi.fn(),
}))

import { invoke } from '@tauri-apps/api/core'
import { authClient } from '../../lib/auth-client'
import { getSubscriptionStatus } from '../../lib/api'
import { toast } from '../../components/Toast'

function getState() {
  return useAuthStore.getState()
}

describe('authStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()

    // Reset store state
    useAuthStore.setState({
      user: null,
      plan: 'free',
      source: 'free',
      displayName: 'Free',
      subscriptionEnd: null,
      subscriptionStatus: null,
      licenseStatus: null,
      quotaModel: 'legacy_dual_meter',
      displayWordsUsedEstimate: 0,
      displayWordsLimit: 0,
      displayWordsResetAt: null,
      sttSecondsUsed: 0,
      sttSecondsLimit: 0,
      llmTokensUsed: 0,
      llmTokensLimit: 0,
      cloudWordsUsed: 0,
      cloudWordsLimit: 0,
      cloudWordsResetAt: null,
      byokUnlimited: true,
      loading: false,
      error: null,
    })

    // Set up mock implementations fresh each test
    vi.mocked(invoke).mockResolvedValue(undefined)
    vi.mocked(authClient.getSession).mockResolvedValue({ data: null } as never)
    vi.mocked(authClient.signIn.email).mockResolvedValue({ data: null, error: null } as never)
    vi.mocked(authClient.signUp.email).mockResolvedValue({ data: null, error: null } as never)
    vi.mocked(authClient.sendVerificationEmail).mockResolvedValue({
      data: null,
      error: null,
    } as never)
    vi.mocked(authClient.signOut).mockResolvedValue(undefined as never)
    vi.mocked(getSubscriptionStatus).mockResolvedValue({
      plan: 'pro',
      source: 'creem',
      displayName: 'Pro',
      subscriptionEnd: '2025-12-31',
      subscriptionStatus: 'active',
      licenseStatus: null,
      quotaModel: 'legacy_dual_meter',
      displayWordsUsedEstimate: 0,
      displayWordsLimit: 100000,
      displayWordsResetAt: '2026-07-01T00:00:00.000Z',
      sttSecondsUsed: 100,
      sttSecondsLimit: 36000,
      llmTokensUsed: 5000,
      llmTokensLimit: 5000000,
      cloudWordsUsed: 0,
      cloudWordsLimit: 0,
      cloudWordsResetAt: null,
      byokUnlimited: true,
    })
  })

  describe('initial state', () => {
    it('starts with no user and free plan', () => {
      expect(getState().user).toBeNull()
      expect(getState().plan).toBe('free')
      expect(getState().loading).toBe(false)
      expect(getState().error).toBeNull()
    })
  })

  describe('email verification callback', () => {
    it('passes a desktop callback URL when signing up', async () => {
      await getState().signUp('test@example.com', 'password123', 'Test', {
        verificationCallbackURL: 'https://www.opentypeless.com/auth/callback?from=desktop',
      })

      expect(authClient.signUp.email).toHaveBeenCalledWith(
        {
          email: 'test@example.com',
          password: 'password123',
          name: 'Test',
          callbackURL: 'https://www.opentypeless.com/auth/callback?from=desktop',
        },
        expect.any(Object),
      )
      expect(getState().emailVerificationPending).toBe(true)
      expect(getState().pendingEmail).toBe('test@example.com')
    })

    it('resends verification with a desktop callback URL for unverified sign-ins', async () => {
      vi.mocked(authClient.signIn.email).mockResolvedValue({
        data: null,
        error: { code: 'EMAIL_NOT_VERIFIED', message: 'Email not verified' },
      } as never)

      await getState().signIn('test@example.com', 'password123', {
        verificationCallbackURL: 'https://www.opentypeless.com/auth/callback?from=desktop',
      })

      expect(authClient.sendVerificationEmail).toHaveBeenCalledWith({
        email: 'test@example.com',
        callbackURL: 'https://www.opentypeless.com/auth/callback?from=desktop',
      })
      expect(getState().emailVerificationPending).toBe(true)
    })

    it('resends pending verification with the supplied callback URL', async () => {
      useAuthStore.setState({ pendingEmail: 'test@example.com' })

      await getState().resendVerification({
        verificationCallbackURL: 'https://www.opentypeless.com/auth/callback?from=desktop',
      })

      expect(authClient.sendVerificationEmail).toHaveBeenCalledWith({
        email: 'test@example.com',
        callbackURL: 'https://www.opentypeless.com/auth/callback?from=desktop',
      })
    })
  })

  describe('signOut', () => {
    it('clears user and resets to free plan', async () => {
      useAuthStore.setState({
        user: { id: '1', email: 'test@example.com', name: 'Test' },
        plan: 'pro',
        source: 'creem',
        displayName: 'Pro',
        subscriptionEnd: '2025-12-31',
        subscriptionStatus: 'active',
        licenseStatus: null,
        quotaModel: 'legacy_dual_meter',
        displayWordsUsedEstimate: 0,
        displayWordsLimit: 0,
        displayWordsResetAt: null,
        sttSecondsUsed: 100,
        sttSecondsLimit: 36000,
        llmTokensUsed: 5000,
        llmTokensLimit: 5000000,
        cloudWordsUsed: 0,
        cloudWordsLimit: 0,
        cloudWordsResetAt: null,
        byokUnlimited: true,
      })

      await getState().signOut()

      expect(getState().user).toBeNull()
      expect(getState().plan).toBe('free')
      expect(getState().subscriptionEnd).toBeNull()
      expect(getState().sttSecondsUsed).toBe(0)
      expect(getState().llmTokensUsed).toBe(0)
    })
  })

  describe('refreshSubscription', () => {
    it('updates quota fields from API response', async () => {
      useAuthStore.setState({
        user: { id: '1', email: 'test@example.com', name: 'Test' },
      })

      await getState().refreshSubscription()

      expect(getState().plan).toBe('pro')
      expect(getState().subscriptionEnd).toBe('2025-12-31')
      expect(getState().sttSecondsUsed).toBe(100)
      expect(getState().sttSecondsLimit).toBe(36000)
      expect(getState().llmTokensUsed).toBe(5000)
      expect(getState().llmTokensLimit).toBe(5000000)
    })

    it('shows cloud words quota warning only once while usage stays high', async () => {
      vi.mocked(getSubscriptionStatus).mockResolvedValue({
        plan: 'appsumo_tier1',
        source: 'appsumo',
        displayName: 'AppSumo Tier 1',
        subscriptionEnd: null,
        subscriptionStatus: null,
        licenseStatus: 'active',
        quotaModel: 'cloud_words',
        displayWordsUsedEstimate: 0,
        displayWordsLimit: 0,
        displayWordsResetAt: null,
        sttSecondsUsed: 0,
        sttSecondsLimit: 0,
        llmTokensUsed: 0,
        llmTokensLimit: 0,
        cloudWordsUsed: 180000,
        cloudWordsLimit: 200000,
        cloudWordsResetAt: null,
        byokUnlimited: true,
      })
      useAuthStore.setState({
        user: { id: '1', email: 'test@example.com', name: 'Test' },
      })

      await getState().refreshSubscription()
      await getState().refreshSubscription()

      expect(toast).toHaveBeenCalledTimes(1)
      expect(toast).toHaveBeenCalledWith('Cloud words are almost used up.', 'error')
    })
  })

  describe('hasManagedCloudAccess', () => {
    it('allows AppSumo lifetime plans with cloud words', () => {
      expect(
        hasManagedCloudAccess({
          plan: 'appsumo_tier1',
          source: 'appsumo',
          cloudWordsLimit: 200000,
          licenseStatus: 'active',
        }),
      ).toBe(true)
    })

    it('allows Creem Pro plans with cloud words', () => {
      expect(
        hasManagedCloudAccess({
          plan: 'pro',
          source: 'creem',
          cloudWordsLimit: 200000,
          licenseStatus: null,
        }),
      ).toBe(true)
    })

    it('allows direct lifetime plans', () => {
      expect(
        hasManagedCloudAccess({
          plan: 'lifetime_starter',
          source: 'lifetime',
          cloudWordsLimit: 0,
          licenseStatus: 'active',
        }),
      ).toBe(true)

      expect(
        hasManagedCloudAccess({
          plan: 'lifetime_starter',
          source: 'lifetime',
          cloudWordsLimit: 100000,
          licenseStatus: null,
        }),
      ).toBe(true)
    })

    it('denies revoked lifetime licenses even when a quota remains', () => {
      expect(
        hasManagedCloudAccess({
          plan: 'appsumo_tier1',
          source: 'appsumo',
          cloudWordsLimit: 200000,
          licenseStatus: 'refunded',
        }),
      ).toBe(false)
    })

    it('denies AppSumo cloud access unless the license is active', () => {
      expect(
        hasManagedCloudAccess({
          plan: 'appsumo_tier1',
          source: 'appsumo',
          cloudWordsLimit: 200000,
          licenseStatus: 'pending',
        }),
      ).toBe(false)

      expect(
        hasManagedCloudAccess({
          plan: 'appsumo_tier1',
          source: 'appsumo',
          cloudWordsLimit: 200000,
          licenseStatus: null,
        }),
      ).toBe(false)
    })
  })

  describe('initialize', () => {
    it('sets loading during initialization', async () => {
      const promise = getState().initialize()
      expect(getState().loading).toBe(true)
      await promise
      expect(getState().loading).toBe(false)
    })

    it('stays null user when no session exists', async () => {
      await getState().initialize()
      expect(getState().user).toBeNull()
    })
  })
})
