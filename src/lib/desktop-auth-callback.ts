import { API_BASE_URL } from './constants'
import { generateOAuthState } from './deep-link'

export function createDesktopAuthCallbackURL(stateTtlMs?: number): string {
  const state = generateOAuthState(stateTtlMs)
  return `${API_BASE_URL}/auth/callback?from=desktop&state=${state}`
}
