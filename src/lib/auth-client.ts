import { createAuthClient } from 'better-auth/client'
import { API_BASE_URL, APP_VERSION_HEADER_VALUE, CLIENT_VERSION_HEADER } from './constants'

const fetchWithToken: typeof fetch = (url, init) => {
  const headers = new Headers(init?.headers)
  if (!headers.has(CLIENT_VERSION_HEADER)) {
    headers.set(CLIENT_VERSION_HEADER, APP_VERSION_HEADER_VALUE)
  }
  const token = localStorage.getItem('session_token')
  if (token) {
    if (!headers.has('Authorization')) {
      headers.set('Authorization', `Bearer ${token}`)
    }
    return fetch(url, { ...init, headers })
  }
  return fetch(url, { ...init, headers })
}

export const authClient = createAuthClient({
  baseURL: API_BASE_URL,
  fetchOptions: {
    customFetchImpl: fetchWithToken,
  },
})
