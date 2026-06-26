import { describe, expect, it } from 'vitest'
import { capsuleErrorKeyFromPayload } from '../capsuleError'

describe('capsuleErrorKeyFromPayload', () => {
  it('keeps structured STT quota errors short', () => {
    expect(
      capsuleErrorKeyFromPayload({
        code: 'stt_quota_exceeded',
        details: 'limit hit',
        retry_count: 0,
      }),
    ).toBe('stt_quota_exceeded')
  })

  it('keeps structured LLM quota errors short', () => {
    expect(
      capsuleErrorKeyFromPayload({
        code: 'llm_quota_exceeded',
        details: 'limit hit',
        retry_count: 0,
      }),
    ).toBe('llm_quota_exceeded')
  })

  it('keeps structured Wayland clipboard copy-only warnings short', () => {
    expect(
      capsuleErrorKeyFromPayload({
        code: 'output_wayland_clipboard_copy_only',
        retry_count: 0,
      }),
    ).toBe('output_wayland_clipboard_copy_only')
  })

  it('maps missing STT setup strings to a setup label', () => {
    expect(
      capsuleErrorKeyFromPayload(
        'STT API key is not configured. Please set it in Settings -> Speech Recognition.',
      ),
    ).toBe('stt_not_configured')
  })

  it('maps custom Whisper setup strings to a setup label', () => {
    expect(capsuleErrorKeyFromPayload('Local / Custom Whisper is missing base URL or model')).toBe(
      'stt_not_configured',
    )
  })

  it('maps accessibility errors to the short permission label', () => {
    expect(capsuleErrorKeyFromPayload('ACCESSIBILITY_REQUIRED')).toBe('accessibility_required')
  })

  it('maps wrapped accessibility output failures to the permission label', () => {
    expect(capsuleErrorKeyFromPayload('Output failed: ACCESSIBILITY_REQUIRED')).toBe(
      'accessibility_required',
    )
  })

  it('keeps structured accessibility errors short', () => {
    expect(
      capsuleErrorKeyFromPayload({
        code: 'accessibility_required',
        retry_count: 0,
      }),
    ).toBe('accessibility_required')
  })

  it('maps output failures to a short output label', () => {
    expect(capsuleErrorKeyFromPayload('Output failed: permission denied')).toBe('output_failed')
  })

  it('falls back safely for unknown structured errors', () => {
    expect(
      capsuleErrorKeyFromPayload({
        code: 'new_backend_error',
        retry_count: 0,
      }),
    ).toBe('unknown')
  })
})
