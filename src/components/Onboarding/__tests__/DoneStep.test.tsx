import { cleanup, render, screen } from '@testing-library/react'
import React from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DoneStep } from '../DoneStep'

vi.mock('framer-motion', () => ({
  motion: new Proxy(
    {},
    {
      get:
        (_target, tag: string) =>
        ({ children, ...props }: any) =>
          React.createElement(tag, props, children),
    },
  ),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'onboarding.done.title': "You're all set!",
        'onboarding.done.capsuleAppearsWhenRecording':
          'The capsule appears while recording and processing.',
        'onboarding.done.holdPress': 'Hold / Press',
        'onboarding.done.holdPressSub': 'Use your hotkey to start and stop recording',
        'onboarding.done.askAnything': 'Ask Anything',
        'onboarding.done.askAnythingSub':
          'Use the Ask hotkey to record a question; stop to get one answer.',
        'onboarding.done.clickCapsule': 'Click Capsule',
        'onboarding.done.clickCapsuleSub': 'When visible, click the capsule to toggle recording.',
        'onboarding.done.dragToReposition': 'Drag to Reposition',
        'onboarding.done.dragToRepositionSub': 'When visible, drag the capsule anywhere on screen.',
        'onboarding.done.rightClickMenu': 'Right-click Menu',
        'onboarding.done.restoreCapsuleSub':
          'Use the tray or Settings to keep the capsule visible while idle.',
      })[key] ?? key,
  }),
}))

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) =>
    selector({
      config: {
        hotkey: 'Option+/',
        ask_hotkey: 'Command+.',
        output_mode: 'clipboard',
      },
    }),
}))

vi.mock('../../../lib/tauri', () => ({
  checkAccessibilityPermission: vi.fn().mockResolvedValue(true),
  requestAccessibilityPermission: vi.fn().mockResolvedValue(true),
  waitForAccessibilityPermission: vi.fn().mockResolvedValue(true),
}))

afterEach(() => cleanup())

describe('DoneStep', () => {
  it('teaches the Ask Anything voice question shortcut during onboarding', () => {
    render(<DoneStep />)

    expect(screen.getByText('Ask Anything Command+.')).toBeInTheDocument()
    expect(
      screen.getByText('Use the Ask hotkey to record a question; stop to get one answer.'),
    ).toBeInTheDocument()
  })
})
