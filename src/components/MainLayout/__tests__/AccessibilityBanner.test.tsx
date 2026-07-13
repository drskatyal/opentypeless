import React from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AccessibilityBanner } from '../AccessibilityBanner'

const mockStore = {
  accessibilityTrusted: false,
  setAccessibilityTrusted: vi.fn(),
  config: {
    hotkey: 'Fn',
    polish_enabled: true,
    context_adaptation_enabled: true,
    output_mode: 'keyboard',
    insertion_strategy: 'auto',
    hotkeys: {
      dictation: { primary: 'Fn', modifiers: [] },
    },
  },
  lastContext: null as any,
  setLastContext: vi.fn(),
}

vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get:
        (_target, tag: string) =>
        ({ children, ...props }: React.HTMLAttributes<HTMLElement>) =>
          React.createElement(tag, props, children),
    },
  ),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'settings.accessibilityRequired': 'Required for Fn shortcut or keyboard output',
        'settings.accessibilityPermission': 'macOS Accessibility',
        'settings.grantPermission': 'Grant',
        'settings.browserAccessHint': 'Allow browser access for web app modes.',
        'settings.allowBrowserAccess': 'Allow',
        'common.close': 'Close',
      })[key] ?? key,
  }),
}))

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => selector(mockStore),
}))

vi.mock('../../../lib/tauri', () => ({
  checkAccessibilityPermission: vi.fn().mockResolvedValue(false),
  requestAccessibilityPermission: vi.fn().mockResolvedValue(true),
  waitForAccessibilityPermission: vi.fn().mockResolvedValue(true),
  resumeHotkey: vi.fn().mockResolvedValue(undefined),
  requestBrowserAccess: vi.fn().mockResolvedValue('available'),
}))

beforeEach(() => {
  Object.assign(mockStore, {
    accessibilityTrusted: false,
    setAccessibilityTrusted: vi.fn((trusted: boolean) => {
      mockStore.accessibilityTrusted = trusted
    }),
    lastContext: null,
    setLastContext: vi.fn(),
  })
  Object.assign(mockStore.config, {
    hotkey: 'Fn',
    output_mode: 'keyboard',
    insertion_strategy: 'auto',
    hotkeys: {
      dictation: { primary: 'Fn', modifiers: [] },
    },
  })
  Object.defineProperty(window.navigator, 'platform', {
    value: 'MacIntel',
    configurable: true,
  })
})

afterEach(() => cleanup())

describe('AccessibilityBanner', () => {
  it('re-registers hotkeys after Accessibility is granted from the banner', async () => {
    const tauri = await import('../../../lib/tauri')

    render(<AccessibilityBanner />)
    fireEvent.click(screen.getByRole('button', { name: 'Grant' }))

    await waitFor(() => {
      expect(tauri.requestAccessibilityPermission).toHaveBeenCalled()
      expect(tauri.resumeHotkey).toHaveBeenCalled()
      expect(mockStore.setAccessibilityTrusted).toHaveBeenCalledWith(true)
    })
  })

  it('requires Accessibility when clipboard paste simulates the paste shortcut', () => {
    Object.assign(mockStore.config, {
      hotkey: 'Ctrl+/',
      output_mode: 'clipboard',
      insertion_strategy: 'clipboardPaste',
      hotkeys: {
        dictation: { primary: '/', modifiers: ['Ctrl'] },
      },
    })

    render(<AccessibilityBanner />)

    expect(screen.getByRole('button', { name: 'Grant' })).toBeInTheDocument()
  })

  it('does not require Accessibility for copy-only output with a non-Fn shortcut', () => {
    Object.assign(mockStore.config, {
      hotkey: 'Ctrl+/',
      output_mode: 'clipboard',
      insertion_strategy: 'clipboardCopyOnly',
      hotkeys: {
        dictation: { primary: '/', modifiers: ['Ctrl'] },
      },
    })

    render(<AccessibilityBanner />)

    expect(screen.queryByRole('button', { name: 'Grant' })).not.toBeInTheDocument()
  })

  it('still requires Accessibility when Fn is a secondary copy-only shortcut', () => {
    Object.assign(mockStore.config, {
      hotkey: 'Ctrl+/',
      output_mode: 'clipboard',
      insertion_strategy: 'clipboardCopyOnly',
      hotkeys: {
        dictation: { primary: '/', modifiers: ['Ctrl'] },
        dictationBindings: [
          { primary: '/', modifiers: ['Ctrl'] },
          { primary: 'Fn', modifiers: [] },
        ],
      },
    })

    render(<AccessibilityBanner />)

    expect(screen.getByRole('button', { name: 'Grant' })).toBeInTheDocument()
  })

  it('requests access only for the browser captured with the blocked operation', async () => {
    const tauri = await import('../../../lib/tauri')
    Object.assign(mockStore.config, {
      hotkey: 'Ctrl+/',
      output_mode: 'clipboard',
      insertion_strategy: 'clipboardCopyOnly',
      hotkeys: {
        dictation: { primary: '/', modifiers: ['Ctrl'] },
        dictationBindings: [{ primary: '/', modifiers: ['Ctrl'] }],
      },
    })
    mockStore.lastContext = {
      profileId: 'general.browser',
      family: 'general',
      appLabel: 'Browser',
      iconKey: 'general',
      overrideId: null,
      browserAccessStatus: 'needs_permission',
      browserTarget: 'chrome',
    }

    render(<AccessibilityBanner />)
    fireEvent.click(screen.getByRole('button', { name: 'Allow' }))

    await waitFor(() => {
      expect(tauri.requestBrowserAccess).toHaveBeenCalledWith('chrome')
      expect(mockStore.setLastContext).toHaveBeenCalledWith({
        ...mockStore.lastContext,
        browserAccessStatus: 'available',
      })
    })
  })

  it('prioritizes the Accessibility blocker over Browser Access', () => {
    mockStore.lastContext = {
      profileId: 'general.browser',
      browserAccessStatus: 'needs_permission',
      browserTarget: 'chrome',
    }

    render(<AccessibilityBanner />)

    expect(screen.getByRole('button', { name: 'Grant' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Allow' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument()
  })
})
