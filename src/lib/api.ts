import { API_BASE_URL } from './constants'

const DEFAULT_TIMEOUT_MS = 30_000

function authHeaders(): Record<string, string> {
  const token = localStorage.getItem('session_token')
  return token ? { Authorization: `Bearer ${token}` } : {}
}

async function request<T>(
  path: string,
  options?: RequestInit & { timeoutMs?: number },
): Promise<T> {
  const { timeoutMs = DEFAULT_TIMEOUT_MS, ...fetchOptions } = options ?? {}
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)

  try {
    const res = await fetch(`${API_BASE_URL}${path}`, {
      ...fetchOptions,
      credentials: 'include',
      signal: controller.signal,
      headers: {
        'Content-Type': 'application/json',
        ...authHeaders(),
        ...fetchOptions?.headers,
      },
    })

    if (!res.ok) {
      const body = await res.json().catch(() => ({ error: res.statusText }))
      throw new ApiError(res.status, body.error ?? res.statusText)
    }

    return res.json()
  } finally {
    clearTimeout(timer)
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

// Subscription
export type SubscriptionPlan = 'free' | 'pro' | 'appsumo_tier1' | 'appsumo_tier2' | 'appsumo_tier3'

export type SubscriptionSource = 'free' | 'creem' | 'appsumo'
export type LicenseStatus = 'pending' | 'active' | 'refunded' | 'deactivated'

export interface SubscriptionStatus {
  plan: SubscriptionPlan
  source: SubscriptionSource
  displayName: string
  subscriptionEnd: string | null
  subscriptionStatus: string | null
  licenseStatus?: LicenseStatus | null
  sttSecondsUsed: number
  sttSecondsLimit: number
  llmTokensUsed: number
  llmTokensLimit: number
  cloudWordsUsed: number
  cloudWordsLimit: number
  cloudWordsResetAt: string | null
  byokUnlimited: boolean
}

export function getSubscriptionStatus(): Promise<SubscriptionStatus> {
  return request<Partial<SubscriptionStatus>>('/api/subscription/status').then((status) => {
    const plan = (status.plan ?? 'free') as SubscriptionPlan
    const source =
      status.source ?? (plan === 'pro' ? 'creem' : plan.startsWith('appsumo_') ? 'appsumo' : 'free')

    return {
      plan,
      source: source as SubscriptionSource,
      displayName:
        status.displayName ??
        (plan === 'pro' ? 'Pro' : plan === 'free' ? 'Free' : 'AppSumo Lifetime'),
      subscriptionEnd: status.subscriptionEnd ?? null,
      subscriptionStatus: status.subscriptionStatus ?? null,
      licenseStatus: status.licenseStatus ?? null,
      sttSecondsUsed: status.sttSecondsUsed ?? 0,
      sttSecondsLimit: status.sttSecondsLimit ?? 0,
      llmTokensUsed: status.llmTokensUsed ?? 0,
      llmTokensLimit: status.llmTokensLimit ?? 0,
      cloudWordsUsed: status.cloudWordsUsed ?? 0,
      cloudWordsLimit: status.cloudWordsLimit ?? 0,
      cloudWordsResetAt: status.cloudWordsResetAt ?? null,
      byokUnlimited: status.byokUnlimited ?? true,
    }
  })
}

// Checkout
export interface CheckoutResponse {
  url: string
}

export function createCheckout(origin: 'desktop' | 'web' = 'desktop'): Promise<CheckoutResponse> {
  return request('/api/checkout/create', {
    method: 'POST',
    body: JSON.stringify({ origin }),
  })
}

// Proxy STT
export async function proxyStt(audioBlob: Blob, language: string): Promise<{ text: string }> {
  const formData = new FormData()
  formData.append('audio', audioBlob)
  formData.append('language', language)

  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), 60_000)

  try {
    const res = await fetch(`${API_BASE_URL}/api/proxy/stt`, {
      method: 'POST',
      credentials: 'include',
      signal: controller.signal,
      headers: authHeaders(),
      body: formData,
    })

    if (!res.ok) {
      const body = await res.json().catch(() => ({ error: res.statusText }))
      throw new ApiError(res.status, body.error ?? res.statusText)
    }

    return res.json()
  } finally {
    clearTimeout(timer)
  }
}

// Proxy LLM
export function proxyLlm(
  messages: Array<{ role: string; content: string }>,
): Promise<{ text: string }> {
  return request('/api/proxy/llm', {
    method: 'POST',
    body: JSON.stringify({ messages }),
  })
}

// Backup
export function uploadBackup(data: {
  history?: unknown
  dictionary?: unknown
  settings?: unknown
}): Promise<{ success: boolean }> {
  return request('/api/backup/upload', {
    method: 'POST',
    body: JSON.stringify(data),
  })
}

export function downloadBackup(): Promise<{
  history?: unknown
  dictionary?: unknown
  settings?: unknown
}> {
  return request('/api/backup/download')
}

// Scenes
export interface ScenePack {
  id: string
  name: string
  description: string
  category: string
  promptTemplate: string
  dictionaryTerms: Array<{ word: string; pronunciation?: string }>
  isPro: boolean
}

export function getScenes(): Promise<ScenePack[]> {
  return request('/api/scenes')
}

// Subscription portal
export function createPortalSession(): Promise<{ url: string }> {
  return request('/api/subscription/portal', { method: 'POST' })
}
