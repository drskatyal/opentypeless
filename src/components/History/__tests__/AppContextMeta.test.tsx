import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { AppContextMeta } from '../AppContextMeta'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'history.providers.managed_cloud': 'Cloud',
        'history.providers.byok': 'BYOK',
        'history.providers.local': 'Local',
        'history.needsBrowserAccess': 'needs browser access',
      })[key] ?? key,
  }),
}))

describe('AppContextMeta', () => {
  it('renders a fixed 16px app logo, safe label, time, and provider', () => {
    const { container } = render(
      <AppContextMeta
        iconKey="gmail"
        family="email"
        label="Gmail"
        time="09:42"
        providerKind="managed_cloud"
      />,
    )

    const logo = container.querySelector('img')
    expect(logo).not.toBeNull()
    expect(logo).toHaveAttribute('width', '16')
    expect(logo).toHaveAttribute('height', '16')
    expect(screen.getByText('Gmail')).toHaveClass('truncate')
    expect(screen.getByText('09:42')).toBeInTheDocument()
    expect(screen.getByText('Cloud')).toHaveClass('max-[419px]:hidden')
  })

  it('uses an existing family icon when no reviewed bitmap exists', () => {
    const { container } = render(
      <AppContextMeta
        iconKey="extended-app"
        family="developer_collaboration"
        label="Self-hosted Git"
        time="10:10"
        providerKind="byok"
      />,
    )

    expect(container.querySelector('svg')).not.toBeNull()
    expect(screen.getByText('Self-hosted Git')).toHaveClass('min-w-0', 'truncate')
  })

  it('adds a browser access hint only for Browser entries that need URL access', () => {
    render(
      <AppContextMeta
        iconKey="general"
        family="general"
        label="Browser"
        time="10:10"
        providerKind="local"
        browserAccessStatus="needs_permission"
      />,
    )

    expect(screen.getByText('needs browser access')).toBeInTheDocument()
  })
})
