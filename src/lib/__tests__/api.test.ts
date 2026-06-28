import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { ApiError } from '../api'
import { API_BASE_URL, APP_VERSION_HEADER_VALUE, CLIENT_VERSION_HEADER } from '../constants'

const API_BASE = API_BASE_URL

describe('ApiError', () => {
  it('stores status and message', () => {
    const err = new ApiError(404, 'Not found')
    expect(err.status).toBe(404)
    expect(err.message).toBe('Not found')
    expect(err.name).toBe('ApiError')
    expect(err).toBeInstanceOf(Error)
  })
})

describe('request() via getSubscriptionStatus', () => {
  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: () =>
          Promise.resolve({
            plan: 'pro',
            subscriptionEnd: '2025-12-31',
            sttSecondsUsed: 100,
            sttSecondsLimit: 36000,
            llmTokensUsed: 5000,
            llmTokensLimit: 5000000,
          }),
      }),
    )
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('calls fetch with correct URL and options', async () => {
    const { getSubscriptionStatus } = await import('../api')
    await getSubscriptionStatus()

    expect(fetch).toHaveBeenCalledWith(
      `${API_BASE}/api/subscription/status`,
      expect.objectContaining({
        credentials: 'include',
        headers: expect.objectContaining({
          'Content-Type': 'application/json',
          [CLIENT_VERSION_HEADER]: APP_VERSION_HEADER_VALUE,
        }),
      }),
    )
  })

  it('returns parsed JSON on success', async () => {
    const { getSubscriptionStatus } = await import('../api')
    const result = await getSubscriptionStatus()
    expect(result.plan).toBe('pro')
    expect(result.sttSecondsLimit).toBe(36000)
  })
})

describe('request() error handling', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('throws ApiError with body.error on non-ok response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: false,
        status: 403,
        statusText: 'Forbidden',
        json: () => Promise.resolve({ error: 'Subscription required' }),
      }),
    )

    const { getSubscriptionStatus } = await import('../api')
    await expect(getSubscriptionStatus()).rejects.toThrow('Subscription required')
    await expect(getSubscriptionStatus()).rejects.toBeInstanceOf(ApiError)
  })

  it('falls back to statusText when body has no error field', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: false,
        status: 500,
        statusText: 'Internal Server Error',
        json: () => Promise.resolve({}),
      }),
    )

    const { getSubscriptionStatus } = await import('../api')
    await expect(getSubscriptionStatus()).rejects.toThrow('Internal Server Error')
  })
})

describe('createCheckout', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('sends POST with origin and checkout product in body', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ url: 'https://checkout.stripe.com/xxx' }),
      }),
    )

    const { createCheckout } = await import('../api')
    const result = await createCheckout('web', 'lifetime_starter')

    expect(fetch).toHaveBeenCalledWith(
      `${API_BASE}/api/checkout/create`,
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ origin: 'web', product: 'lifetime_starter' }),
      }),
    )
    expect(result.url).toBe('https://checkout.stripe.com/xxx')
  })
})

describe('proxyStt', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('sends the desktop client version without forcing a JSON content type', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ text: 'transcribed text' }),
      }),
    )

    const { proxyStt } = await import('../api')
    const result = await proxyStt(new Blob(['audio'], { type: 'audio/wav' }), 'en')

    expect(fetch).toHaveBeenCalledWith(
      `${API_BASE}/api/proxy/stt`,
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          [CLIENT_VERSION_HEADER]: APP_VERSION_HEADER_VALUE,
        }),
      }),
    )
    const headers = vi.mocked(fetch).mock.calls[0][1]?.headers as Record<string, string>
    expect(headers['Content-Type']).toBeUndefined()
    expect(result.text).toBe('transcribed text')
  })
})

describe('proxyLlm', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('sends messages array as POST body', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve({ text: 'polished text' }),
      }),
    )

    const { proxyLlm } = await import('../api')
    const messages = [{ role: 'user', content: 'hello' }]
    const result = await proxyLlm(messages)

    expect(fetch).toHaveBeenCalledWith(
      `${API_BASE}/api/proxy/llm`,
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({
          [CLIENT_VERSION_HEADER]: APP_VERSION_HEADER_VALUE,
        }),
        body: JSON.stringify({ messages }),
      }),
    )
    expect(result.text).toBe('polished text')
  })
})
