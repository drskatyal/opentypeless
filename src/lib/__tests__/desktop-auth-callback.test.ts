import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createDesktopAuthCallbackURL } from '../desktop-auth-callback'

describe('createDesktopAuthCallbackURL', () => {
  beforeEach(() => {
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'desktop-state-1') })
    localStorage.clear()
  })

  it('uses one callback query parameter so email verification links keep state intact', () => {
    expect(createDesktopAuthCallbackURL()).toBe(
      'https://www.opentypeless.com/auth/callback?desktop=desktop-state-1',
    )
  })
})
