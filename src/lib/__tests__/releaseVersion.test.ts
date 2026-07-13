import { describe, expect, it } from 'vitest'
import constantsSource from '../constants.ts?raw'
import windowsCertificateScriptSource from '../../../.github/scripts/import-windows-certificate.ps1?raw'
import releaseWorkflowSource from '../../../.github/workflows/release.yml?raw'

describe('release version wiring', () => {
  it('lets frontend builds read the release tag version from Vite env', () => {
    expect(constantsSource).toContain('import.meta.env.VITE_APP_VERSION')
  })

  it('exports VITE_APP_VERSION during the GitHub release build', () => {
    expect(releaseWorkflowSource).toContain('VITE_APP_VERSION=v$VERSION')
  })

  it('creates cross-repository releases from the owner main branch', () => {
    expect(releaseWorkflowSource).toContain('releaseCommitish: main')
    expect(releaseWorkflowSource).not.toContain('releaseCommitish: ${{ github.sha }}')
  })

  it('requires an explicit opt-in before publishing unsigned Windows installers', () => {
    expect(releaseWorkflowSource).toContain('allow_unsigned_windows:')
    expect(releaseWorkflowSource).toContain(
      'ALLOW_UNSIGNED_WINDOWS: ${{ github.event.inputs.allow_unsigned_windows }}',
    )
    expect(windowsCertificateScriptSource).toContain(
      "$allowUnsigned = $env:ALLOW_UNSIGNED_WINDOWS -eq 'true'",
    )
    expect(windowsCertificateScriptSource).toContain(
      'Unsigned Windows release explicitly allowed for this manual dispatch.',
    )
  })
})
