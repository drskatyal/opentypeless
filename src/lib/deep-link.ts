import { onOpenUrl } from '@tauri-apps/plugin-deep-link'
import { useAuthStore } from '../stores/authStore'

/** Pending OAuth state for CSRF validation. */
let pendingOAuthState: string | null = null
let pendingOAuthTimer: ReturnType<typeof setTimeout> | null = null
const OAUTH_STATE_STORAGE_KEY = 'opentypeless.pendingOAuthState'
const OAUTH_STATE_TTL_MS = 5 * 60 * 1000

function persistOAuthState(state: string): void {
  try {
    localStorage.setItem(
      OAUTH_STATE_STORAGE_KEY,
      JSON.stringify({ state, expiresAt: Date.now() + OAUTH_STATE_TTL_MS }),
    )
  } catch {
    // localStorage may be unavailable in some webview/test contexts.
  }
}

function loadPersistedOAuthState(): string | null {
  try {
    const raw = localStorage.getItem(OAUTH_STATE_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as { state?: unknown; expiresAt?: unknown }
    if (typeof parsed.state !== 'string' || typeof parsed.expiresAt !== 'number') {
      localStorage.removeItem(OAUTH_STATE_STORAGE_KEY)
      return null
    }
    if (Date.now() > parsed.expiresAt) {
      localStorage.removeItem(OAUTH_STATE_STORAGE_KEY)
      return null
    }
    return parsed.state
  } catch {
    return null
  }
}

/** Generate and store a random state string for OAuth CSRF protection. */
export function generateOAuthState(): string {
  clearOAuthState()
  const state = crypto.randomUUID()
  pendingOAuthState = state
  persistOAuthState(state)
  // Auto-expire after 5 minutes to prevent stale state
  pendingOAuthTimer = setTimeout(clearOAuthState, OAUTH_STATE_TTL_MS)
  return state
}

/** Clear pending OAuth state (e.g. user cancelled or timed out). */
export function clearOAuthState(): void {
  pendingOAuthState = null
  try {
    localStorage.removeItem(OAUTH_STATE_STORAGE_KEY)
  } catch {
    // localStorage may be unavailable in some webview/test contexts.
  }
  if (pendingOAuthTimer) {
    clearTimeout(pendingOAuthTimer)
    pendingOAuthTimer = null
  }
}

export async function initDeepLinkListener() {
  try {
    await onOpenUrl(async (urls) => {
      for (const rawUrl of urls) {
        await handleDeepLinkUrl(rawUrl)
      }
    })
  } catch {
    // Deep link plugin not available (e.g. web dev mode)
  }
}

/** Basic sanity check: token must be a non-empty alphanumeric/JWT-like string. */
function isValidToken(token: string): boolean {
  return /^[\w\-._~+/]+=*$/.test(token) && token.length >= 10 && token.length <= 4096
}

export async function handleDeepLinkUrl(rawUrl: string): Promise<boolean> {
  console.log('[deep-link] received URL:', rawUrl.replace(/token=[^&]+/, 'token=***'))
  let url: URL
  try {
    url = new URL(rawUrl)
  } catch {
    return false
  }

  // Only accept our custom scheme
  if (url.protocol !== 'opentypeless:') return false

  const path = url.hostname + url.pathname
  const params = url.searchParams

  // opentypeless://auth/callback?token=xxx&state=yyy
  if (path === 'auth/callback' || path === 'auth/callback/') {
    const token = params.get('token')
    const state = params.get('state')
    const expectedState = pendingOAuthState ?? loadPersistedOAuthState()

    // Reject tokens when no OAuth flow was initiated (prevents external injection)
    if (!expectedState) {
      return false
    }
    // Validate CSRF state
    if (state !== expectedState) {
      clearOAuthState()
      return false
    }
    clearOAuthState()

    if (token && isValidToken(token)) {
      await useAuthStore.getState().handleDeepLinkToken(token)
      window.location.hash = '#/account'
      return true
    }
    return false
  }

  // opentypeless://checkout/success
  if (path === 'checkout/success' || path === 'checkout/success/') {
    await useAuthStore.getState().refreshSubscription()
    window.location.hash = '#/upgrade'
    return true
  }

  return false
}
