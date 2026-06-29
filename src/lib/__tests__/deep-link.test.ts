import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  deepLinkHandler: null as null | ((urls: string[]) => Promise<void> | void),
  handleDeepLinkToken: vi.fn(),
  refreshSubscription: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-deep-link', () => ({
  onOpenUrl: vi.fn(async (handler: (urls: string[]) => Promise<void> | void) => {
    mocks.deepLinkHandler = handler
  }),
}))

vi.mock('../../stores/authStore', () => ({
  useAuthStore: {
    getState: () => ({
      handleDeepLinkToken: mocks.handleDeepLinkToken,
      refreshSubscription: mocks.refreshSubscription,
    }),
  },
}))

describe('deep-link OAuth callback', () => {
  beforeEach(() => {
    vi.resetModules()
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'oauth-state-1') })
    localStorage.clear()
    window.location.hash = ''
    mocks.deepLinkHandler = null
    mocks.handleDeepLinkToken.mockReset()
    mocks.refreshSubscription.mockReset()
  })

  it('accepts the callback after a reload loses in-memory OAuth state', async () => {
    const firstModule = await import('../deep-link')
    const state = firstModule.generateOAuthState()

    vi.resetModules()
    const secondModule = await import('../deep-link')
    await secondModule.initDeepLinkListener()
    await mocks.deepLinkHandler?.([
      `opentypeless://auth/callback?token=valid-token-12345&state=${state}`,
    ])

    expect(mocks.handleDeepLinkToken).toHaveBeenCalledWith('valid-token-12345')
    expect(window.location.hash).toBe('#/account')
  })

  it('returns true when a pasted desktop callback signs the user in', async () => {
    const module = await import('../deep-link')
    const state = module.generateOAuthState()

    const handled = await module.handleDeepLinkUrl(
      `opentypeless://auth/callback?token=valid-token-12345&state=${state}`,
    )

    expect(handled).toBe(true)
    expect(mocks.handleDeepLinkToken).toHaveBeenCalledWith('valid-token-12345')
    expect(window.location.hash).toBe('#/account')
  })
})
