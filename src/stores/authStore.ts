import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { authClient } from '../lib/auth-client'
import {
  getSubscriptionStatus,
  type LicenseStatus,
  type SubscriptionPlan,
  type SubscriptionSource,
} from '../lib/api'
import { toast } from '../components/Toast'
import i18n from '../i18n'

let sttWarningShown = false
let llmWarningShown = false
let cloudWordsWarningShown = false

export interface AuthUser {
  id: string
  email: string
  name: string | null
}

interface AuthState {
  // User
  user: AuthUser | null
  plan: SubscriptionPlan
  source: SubscriptionSource
  displayName: string
  subscriptionEnd: string | null
  subscriptionStatus: string | null
  licenseStatus: LicenseStatus | null

  // Quotas
  sttSecondsUsed: number
  sttSecondsLimit: number
  llmTokensUsed: number
  llmTokensLimit: number
  cloudWordsUsed: number
  cloudWordsLimit: number
  cloudWordsResetAt: string | null
  byokUnlimited: boolean

  // Loading
  loading: boolean
  error: string | null
  emailVerificationPending: boolean
  pendingEmail: string | null

  // Checkout flow
  checkoutPending: boolean

  // Actions
  initialize: () => Promise<void>
  signIn: (email: string, password: string) => Promise<void>
  signUp: (email: string, password: string, name: string) => Promise<void>
  resendVerification: () => Promise<void>
  signOut: () => Promise<void>
  refreshSubscription: () => Promise<void>
  handleDeepLinkToken: (token: string) => Promise<void>
}

export function hasManagedCloudAccess(
  state: Pick<AuthState, 'plan' | 'source' | 'cloudWordsLimit' | 'licenseStatus'>,
): boolean {
  if (state.licenseStatus === 'refunded' || state.licenseStatus === 'deactivated') return false
  if (state.source === 'appsumo') {
    return state.cloudWordsLimit > 0 && state.licenseStatus === 'active'
  }
  if (state.source === 'creem' && state.cloudWordsLimit > 0) {
    return true
  }
  return state.plan === 'pro'
}

export const useAuthStore = create<AuthState>((set, get) => ({
  user: null,
  plan: 'free',
  source: 'free',
  displayName: 'Free',
  subscriptionEnd: null,
  subscriptionStatus: null,
  licenseStatus: null,
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
  emailVerificationPending: false,
  pendingEmail: null,
  checkoutPending: false,

  initialize: async () => {
    try {
      set({ loading: true, error: null })
      const { data: session } = await authClient.getSession()
      if (session?.user) {
        set({
          user: {
            id: session.user.id,
            email: session.user.email,
            name: session.user.name ?? null,
          },
        })
        // Push saved session token to Rust for cloud providers
        const savedToken = localStorage.getItem('session_token')
        if (savedToken) {
          await invoke('set_session_token', { token: savedToken }).catch((e) => {
            console.error('Failed to sync session token to backend:', e)
          })
        }
        await get().refreshSubscription()
      }
    } catch {
      // Not logged in — that's fine
    } finally {
      set({ loading: false })
    }
  },

  signIn: async (email, password) => {
    set({ loading: true, error: null })
    try {
      const { data, error } = await authClient.signIn.email(
        { email, password },
        {
          onSuccess: async (ctx) => {
            const token = ctx.response.headers.get('set-auth-token')
            if (token) {
              localStorage.setItem('session_token', token)
              await invoke('set_session_token', { token }).catch((e: unknown) => {
                console.error('Failed to sync session token to backend:', e)
              })
            }
          },
        },
      )
      if (error) {
        if (error.code === 'EMAIL_NOT_VERIFIED') {
          set({ emailVerificationPending: true, pendingEmail: email })
          return
        }
        throw new Error(error.message ?? 'Sign in failed')
      }
      if (data?.user) {
        set({
          user: {
            id: data.user.id,
            email: data.user.email,
            name: data.user.name ?? null,
          },
        })
        await get().refreshSubscription()
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Sign in failed'
      set({ error: msg })
      throw e
    } finally {
      set({ loading: false })
    }
  },

  signUp: async (email, password, name) => {
    set({ loading: true, error: null, emailVerificationPending: false })
    try {
      const { error } = await authClient.signUp.email(
        { email, password, name },
        {
          onSuccess: async (ctx) => {
            const token = ctx.response.headers.get('set-auth-token')
            if (token) {
              localStorage.setItem('session_token', token)
              await invoke('set_session_token', { token }).catch((e: unknown) => {
                console.error('Failed to sync session token to backend:', e)
              })
            }
          },
        },
      )
      if (error) throw new Error(error.message ?? 'Sign up failed')
      // Email verification is required — don't set user yet
      set({ emailVerificationPending: true, pendingEmail: email })
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Sign up failed'
      set({ error: msg })
      throw e
    } finally {
      set({ loading: false })
    }
  },

  resendVerification: async () => {
    const email = get().pendingEmail
    if (!email) return
    set({ loading: true, error: null })
    try {
      const { error } = await authClient.sendVerificationEmail({ email })
      if (error) throw new Error(error.message ?? 'Failed to resend')
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to resend verification email'
      set({ error: msg })
    } finally {
      set({ loading: false })
    }
  },

  signOut: async () => {
    try {
      await authClient.signOut()
    } finally {
      // Clear session token in localStorage and Rust
      localStorage.removeItem('session_token')
      await invoke('set_session_token', { token: '' }).catch((e: unknown) => {
        console.error('Failed to clear session token in backend:', e)
      })
      set({
        user: null,
        plan: 'free',
        source: 'free',
        displayName: 'Free',
        subscriptionEnd: null,
        subscriptionStatus: null,
        licenseStatus: null,
        sttSecondsUsed: 0,
        sttSecondsLimit: 0,
        llmTokensUsed: 0,
        llmTokensLimit: 0,
        cloudWordsUsed: 0,
        cloudWordsLimit: 0,
        cloudWordsResetAt: null,
        byokUnlimited: true,
        error: null,
        emailVerificationPending: false,
        pendingEmail: null,
        checkoutPending: false,
      })
      sttWarningShown = false
      llmWarningShown = false
      cloudWordsWarningShown = false
    }
  },

  refreshSubscription: async () => {
    try {
      const status = await getSubscriptionStatus()
      set({
        plan: status.plan,
        source: status.source,
        displayName: status.displayName,
        subscriptionEnd: status.subscriptionEnd,
        subscriptionStatus: status.subscriptionStatus,
        licenseStatus: status.licenseStatus ?? null,
        sttSecondsUsed: status.sttSecondsUsed,
        sttSecondsLimit: status.sttSecondsLimit,
        llmTokensUsed: status.llmTokensUsed,
        llmTokensLimit: status.llmTokensLimit,
        cloudWordsUsed: status.cloudWordsUsed,
        cloudWordsLimit: status.cloudWordsLimit,
        cloudWordsResetAt: status.cloudWordsResetAt,
        byokUnlimited: status.byokUnlimited,
      })
      // Clear checkout pending flag after first post-checkout refresh
      if (get().checkoutPending) {
        set({ checkoutPending: false })
      }
      if (status.cloudWordsLimit > 0 && status.cloudWordsUsed / status.cloudWordsLimit >= 0.9) {
        if (!cloudWordsWarningShown) {
          toast(i18n.t('account.cloudQuotaWarning', 'Cloud words are almost used up.'), 'error')
          cloudWordsWarningShown = true
        }
        sttWarningShown = true
        llmWarningShown = true
      } else {
        cloudWordsWarningShown = false
      }
      if (
        status.sttSecondsLimit > 0 &&
        status.sttSecondsUsed / status.sttSecondsLimit >= 0.9 &&
        !sttWarningShown
      ) {
        toast(i18n.t('account.sttQuotaWarning'), 'error')
        sttWarningShown = true
      }
      if (
        status.llmTokensLimit > 0 &&
        status.llmTokensUsed / status.llmTokensLimit >= 0.9 &&
        !llmWarningShown
      ) {
        toast(i18n.t('account.llmQuotaWarning'), 'error')
        llmWarningShown = true
      }
    } catch (e) {
      console.warn('Failed to refresh subscription status:', e instanceof Error ? e.message : e)
    }
  },

  handleDeepLinkToken: async (token: string) => {
    try {
      set({ loading: true, error: null })
      localStorage.setItem('session_token', token)
      await invoke('set_session_token', { token }).catch((e: unknown) => {
        console.error('Failed to sync session token to backend:', e)
      })
      const { data: session } = await authClient.getSession({
        fetchOptions: {
          headers: { Authorization: `Bearer ${token}` },
        },
      })
      if (session?.user) {
        set({
          user: {
            id: session.user.id,
            email: session.user.email,
            name: session.user.name ?? null,
          },
        })
        await get().refreshSubscription()
      }
    } catch {
      set({ error: 'Failed to authenticate with token' })
    } finally {
      set({ loading: false })
    }
  },
}))
