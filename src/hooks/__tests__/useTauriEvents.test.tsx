import { act, cleanup, render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useTauriEvents } from '../useTauriEvents'
import { useAppStore } from '../../stores/appStore'

const eventListeners = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>())

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    eventListeners.set(event, handler)
    return Promise.resolve(vi.fn())
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('../../i18n', () => ({
  default: {
    language: 'en',
    changeLanguage: vi.fn(),
  },
}))

vi.mock('../../lib/tauri', () => ({
  getHistory: vi.fn().mockResolvedValue([]),
}))

vi.mock('../../components/Toast', () => ({
  toast: vi.fn(),
}))

function HookHarness() {
  useTauriEvents()
  return null
}

describe('useTauriEvents', () => {
  beforeEach(() => {
    eventListeners.clear()
    useAppStore.setState({ hotkeyRegistrationError: null })
  })

  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  it('clears hotkey registration errors when the backend reports recovery', async () => {
    render(<HookHarness />)

    await waitFor(() => {
      expect(eventListeners.has('hotkey:registration-failed')).toBe(true)
    })

    act(() => {
      eventListeners.get('hotkey:registration-failed')?.({ payload: 'Shortcut occupied' })
    })
    expect(useAppStore.getState().hotkeyRegistrationError).toBe('Shortcut occupied')

    await waitFor(() => {
      expect(eventListeners.has('hotkey:registration-recovered')).toBe(true)
    })

    act(() => {
      eventListeners.get('hotkey:registration-recovered')?.({ payload: undefined })
    })
    expect(useAppStore.getState().hotkeyRegistrationError).toBeNull()
  })

  it('clears stale capsule errors when a new pipeline run starts preparing', async () => {
    useAppStore.setState({ pipelineError: 'Previous failure' })
    render(<HookHarness />)

    await waitFor(() => {
      expect(eventListeners.has('pipeline:state')).toBe(true)
    })

    act(() => {
      eventListeners.get('pipeline:state')?.({ payload: 'preparing' })
    })

    expect(useAppStore.getState().pipelineError).toBeNull()
  })
})
