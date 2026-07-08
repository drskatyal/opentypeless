import { describe, expect, it } from 'vitest'
import type { CustomScene } from '../../../stores/appStore'
import { importCustomScenesJson, serializeCustomScenes } from '../sceneImportExport'

const existingScene: CustomScene = {
  id: 'custom_existing',
  name: 'Existing',
  description: 'Already present',
  prompt_template: 'Keep as-is.',
  created_at: '2026-07-01T00:00:00.000Z',
  updated_at: '2026-07-01T00:00:00.000Z',
}

describe('scene import/export', () => {
  it('serializes custom scenes into a portable versioned JSON payload', () => {
    const json = serializeCustomScenes([existingScene], '2026-07-06T00:00:00.000Z')
    const payload = JSON.parse(json)

    expect(payload.version).toBe(1)
    expect(payload.exportedAt).toBe('2026-07-06T00:00:00.000Z')
    expect(payload.scenes).toEqual([
      expect.objectContaining({
        id: 'custom_existing',
        name: 'Existing',
        description: 'Already present',
        promptTemplate: 'Keep as-is.',
        source: 'custom',
      }),
    ])
  })

  it('imports style-pack JSON without overwriting existing scene ids', () => {
    const input = JSON.stringify({
      version: 1,
      scenes: [
        {
          id: 'custom_existing',
          name: ' Imported support reply ',
          description: ' Reply template ',
          promptTemplate: ' Write a concise support response. ',
        },
      ],
    })

    const result = importCustomScenesJson(input, {
      existingIds: new Set(['custom_existing']),
      createId: () => 'custom_imported_1',
      nowIso: () => '2026-07-06T01:00:00.000Z',
    })

    expect(result.skipped).toBe(0)
    expect(result.scenes).toEqual([
      {
        id: 'custom_imported_1',
        name: 'Imported support reply',
        description: 'Reply template',
        prompt_template: 'Write a concise support response.',
        created_at: '2026-07-06T01:00:00.000Z',
        updated_at: '2026-07-06T01:00:00.000Z',
      },
    ])
  })

  it('skips invalid scenes and bounds imported field sizes', () => {
    const input = JSON.stringify({
      version: 1,
      scenes: [
        { id: 'empty_prompt', name: 'No prompt', promptTemplate: '   ' },
        {
          id: 'valid',
          name: `  Name\0${'x'.repeat(120)}  `,
          description: `  Desc\0${'y'.repeat(300)}  `,
          promptTemplate: `  Prompt\0${'z'.repeat(5000)}  `,
        },
      ],
    })

    const result = importCustomScenesJson(input, {
      existingIds: new Set(),
      createId: () => 'custom_unused',
      nowIso: () => '2026-07-06T02:00:00.000Z',
    })

    expect(result.skipped).toBe(1)
    expect(result.scenes).toHaveLength(1)
    expect(result.scenes[0].id).toBe('valid')
    expect(result.scenes[0].name).not.toContain('\0')
    expect(result.scenes[0].name).toHaveLength(80)
    expect(result.scenes[0].description).toHaveLength(240)
    expect(result.scenes[0].prompt_template).toHaveLength(4000)
  })

  it('reports import counts, renamed conflicts, invalid rows, and limit skips', () => {
    const validScenes = Array.from({ length: 101 }, (_, index) => ({
      id: `custom_valid_${index}`,
      name: `Valid ${index}`,
      promptTemplate: `Use style ${index}.`,
    }))
    const input = JSON.stringify({
      version: 1,
      scenes: [
        {
          id: 'custom_existing',
          name: 'Conflicting scene',
          promptTemplate: 'Keep this imported scene.',
        },
        { id: 'invalid', name: 'Invalid scene', promptTemplate: '   ' },
        ...validScenes,
      ],
    })

    const result = importCustomScenesJson(input, {
      existingIds: new Set(['custom_existing']),
      createId: () => 'custom_imported_1',
      nowIso: () => '2026-07-06T03:00:00.000Z',
    })

    expect(result.skipped).toBe(4)
    expect(result.report).toEqual({
      totalRecords: 103,
      imported: 99,
      skippedInvalid: 1,
      skippedLimit: 3,
      renamedConflicts: 1,
    })
  })
})
