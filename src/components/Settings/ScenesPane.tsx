import { useState, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Copy, Check, Plus, Trash2, Pencil, X, Download, Upload } from 'lucide-react'
import {
  useAppStore,
  type ActiveScene,
  type AppConfig,
  type CustomScene,
} from '../../stores/appStore'
import { updateConfig as persistConfig } from '../../lib/tauri'
import { BUILTIN_SCENES, type BuiltInScene } from '../../lib/scenes/builtinScenes'
import { importCustomScenesJson, serializeCustomScenes } from '../../lib/scenes/sceneImportExport'

interface EditorState {
  mode: 'create' | 'edit'
  id: string | null
  name: string
  description: string
  promptTemplate: string
}

const emptyEditor = (): EditorState => ({
  mode: 'create',
  id: null,
  name: '',
  description: '',
  promptTemplate: '',
})

function createSceneId() {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `custom_${crypto.randomUUID()}`
  }
  return `custom_${Date.now()}`
}

function nowIso() {
  return new Date().toISOString()
}

function readFileText(file: File): Promise<string> {
  if (typeof file.text === 'function') {
    return file.text()
  }

  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '')
    reader.onerror = () => reject(reader.error)
    reader.readAsText(file)
  })
}

function customSceneToActive(scene: CustomScene): ActiveScene {
  return {
    id: scene.id,
    source: 'custom',
    name: scene.name,
    prompt_template: scene.prompt_template,
  }
}

function builtInSceneToActive(scene: BuiltInScene, name: string): ActiveScene {
  return {
    id: scene.id,
    source: 'builtin',
    name,
    prompt_template: scene.promptTemplate,
  }
}

export function ScenesPane() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const setConfig = useAppStore((s) => s.setConfig)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)

  const [copiedId, setCopiedId] = useState<string | null>(null)
  const [mergeMsg, setMergeMsg] = useState<string | null>(null)
  const [mergeOk, setMergeOk] = useState(false)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const importInputRef = useRef<HTMLInputElement | null>(null)

  const saveNextConfig = async (nextConfig: AppConfig) => {
    const previousConfig = useAppStore.getState().config
    const previousSavedConfig = useAppStore.getState().savedConfig
    setConfig(nextConfig)
    setSaveError(null)
    try {
      await persistConfig(nextConfig)
      setSavedConfig(nextConfig)
    } catch {
      setConfig(previousConfig)
      if (previousSavedConfig) setSavedConfig(previousSavedConfig)
      setSaveError(t('scenes.failedToSave'))
    }
  }

  const handleStartCreate = () => {
    setEditor(emptyEditor())
  }

  const handleStartEdit = (scene: CustomScene) => {
    setEditor({
      mode: 'edit',
      id: scene.id,
      name: scene.name,
      description: scene.description,
      promptTemplate: scene.prompt_template,
    })
  }

  const handleSaveEditor = async (activate: boolean) => {
    if (!editor) return
    const name = editor.name.trim()
    const description = editor.description.trim()
    const promptTemplate = editor.promptTemplate.trim()
    if (!name || !promptTemplate) return

    const timestamp = nowIso()
    let nextScenes: CustomScene[]
    let savedScene: CustomScene

    if (editor.mode === 'edit' && editor.id) {
      const existing = config.custom_scenes.find((scene) => scene.id === editor.id)
      savedScene = {
        id: editor.id,
        name,
        description,
        prompt_template: promptTemplate,
        created_at: existing?.created_at ?? timestamp,
        updated_at: timestamp,
      }
      nextScenes = config.custom_scenes.map((scene) =>
        scene.id === editor.id ? savedScene : scene,
      )
    } else {
      savedScene = {
        id: createSceneId(),
        name,
        description,
        prompt_template: promptTemplate,
        created_at: timestamp,
        updated_at: timestamp,
      }
      nextScenes = [...config.custom_scenes, savedScene]
    }

    const wasActive =
      config.active_scene?.source === 'custom' && config.active_scene.id === savedScene.id
    const nextConfig = {
      ...config,
      custom_scenes: nextScenes,
      active_scene: activate || wasActive ? customSceneToActive(savedScene) : config.active_scene,
    }

    await saveNextConfig(nextConfig)
    setEditor(null)
  }

  const handleActivateCustom = async (scene: CustomScene) => {
    await saveNextConfig({ ...config, active_scene: customSceneToActive(scene) })
  }

  const handleActivateBuiltIn = async (scene: BuiltInScene) => {
    await saveNextConfig({
      ...config,
      active_scene: builtInSceneToActive(scene, t(scene.nameKey)),
    })
  }

  const handleDuplicateBuiltIn = async (scene: BuiltInScene) => {
    const timestamp = nowIso()
    const customScene: CustomScene = {
      id: createSceneId(),
      name: t(scene.nameKey),
      description: t(scene.descriptionKey),
      prompt_template: scene.promptTemplate,
      created_at: timestamp,
      updated_at: timestamp,
    }
    await saveNextConfig({
      ...config,
      custom_scenes: [...config.custom_scenes, customScene],
    })
  }

  const handleDuplicateCustom = async (scene: CustomScene) => {
    const timestamp = nowIso()
    const copy: CustomScene = {
      ...scene,
      id: createSceneId(),
      name: t('scenes.copyName', { name: scene.name }),
      created_at: timestamp,
      updated_at: timestamp,
    }
    await saveNextConfig({
      ...config,
      custom_scenes: [...config.custom_scenes, copy],
    })
  }

  const handleDeleteCustom = async (scene: CustomScene) => {
    if (!window.confirm(t('scenes.deleteConfirm'))) return
    await saveNextConfig({
      ...config,
      custom_scenes: config.custom_scenes.filter((item) => item.id !== scene.id),
      active_scene:
        config.active_scene?.source === 'custom' && config.active_scene.id === scene.id
          ? null
          : config.active_scene,
    })
  }

  const handleClearActive = async () => {
    await saveNextConfig({ ...config, active_scene: null })
  }

  const handleExportCustomScenes = () => {
    const json = serializeCustomScenes(config.custom_scenes)
    const blob = new Blob([json], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `opentypeless-scenes-${new Date().toISOString().slice(0, 10)}.json`
    link.click()
    URL.revokeObjectURL(url)
  }

  const handleImportCustomScenes = async (file: File | undefined) => {
    if (!file) return
    setSaveError(null)
    setMergeMsg(null)
    setMergeOk(false)

    try {
      const json = await readFileText(file)
      const result = importCustomScenesJson(json, {
        existingIds: new Set(config.custom_scenes.map((scene) => scene.id)),
        createId: createSceneId,
        nowIso,
      })

      if (result.scenes.length === 0) {
        setSaveError(t('scenes.importNothing'))
        return
      }

      await saveNextConfig({
        ...config,
        custom_scenes: [...config.custom_scenes, ...result.scenes],
      })
      const importNotes = [
        result.report.skippedInvalid > 0
          ? t('scenes.importSkippedInvalid', { count: result.report.skippedInvalid })
          : null,
        result.report.skippedLimit > 0
          ? t('scenes.importSkippedLimit', { count: result.report.skippedLimit })
          : null,
        result.report.renamedConflicts > 0
          ? t('scenes.importRenamedConflicts', { count: result.report.renamedConflicts })
          : null,
      ].filter(Boolean)
      setMergeOk(true)
      setMergeMsg(
        [
          t('scenes.importedScenes', { count: result.scenes.length }),
          importNotes.length > 0 ? importNotes.join(' · ') : null,
        ]
          .filter(Boolean)
          .join(' · '),
      )
      setTimeout(() => {
        setMergeMsg(null)
        setMergeOk(false)
      }, 3000)
    } catch {
      setSaveError(t('scenes.importFailed'))
    }
  }

  const handleCopyPrompt = async (id: string, promptTemplate: string) => {
    try {
      await navigator.clipboard.writeText(promptTemplate)
      setCopiedId(id)
      setTimeout(() => setCopiedId(null), 2000)
    } catch {
      // Clipboard write failed silently
    }
  }

  return (
    <div className="space-y-6">
      <section className="space-y-3">
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-[13px] font-semibold text-text-primary">{t('scenes.myScenes')}</h3>
          <div className="flex items-center gap-2">
            <button
              onClick={handleExportCustomScenes}
              disabled={config.custom_scenes.length === 0}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-[8px] border border-border bg-transparent text-text-secondary text-[12px] cursor-pointer hover:text-text-primary hover:border-border-focus transition-colors disabled:opacity-45 disabled:cursor-not-allowed"
            >
              <Download size={13} />
              {t('scenes.export')}
            </button>
            <button
              onClick={() => importInputRef.current?.click()}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-[8px] border border-border bg-transparent text-text-secondary text-[12px] cursor-pointer hover:text-text-primary hover:border-border-focus transition-colors"
            >
              <Upload size={13} />
              {t('scenes.import')}
            </button>
            <input
              ref={importInputRef}
              type="file"
              accept="application/json,.json"
              aria-label={t('scenes.import')}
              className="hidden"
              onChange={(event) => {
                const file = event.target.files?.[0]
                event.target.value = ''
                void handleImportCustomScenes(file)
              }}
            />
            <button
              onClick={handleStartCreate}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-[8px] border border-border bg-bg-secondary text-text-primary text-[12px] cursor-pointer hover:border-border-focus transition-colors"
            >
              <Plus size={13} />
              {t('scenes.newScene')}
            </button>
          </div>
        </div>

        {config.active_scene && (
          <div className="flex items-center justify-between gap-3 rounded-[8px] border border-border bg-bg-secondary px-3 py-2">
            <span className="text-[12px] text-text-secondary">
              {t('scenes.activeScene', { name: config.active_scene.name })}
            </span>
            <button
              onClick={handleClearActive}
              className="flex items-center gap-1 text-[12px] text-text-tertiary bg-transparent border-none cursor-pointer hover:text-text-primary transition-colors"
            >
              <X size={12} />
              {t('scenes.clearActive')}
            </button>
          </div>
        )}

        {editor && (
          <SceneEditor
            editor={editor}
            onChange={setEditor}
            onCancel={() => setEditor(null)}
            onSave={() => handleSaveEditor(false)}
            onSaveAndActivate={() => handleSaveEditor(true)}
          />
        )}

        {config.custom_scenes.length === 0 ? (
          <div className="rounded-[8px] border border-dashed border-border px-4 py-6 text-center">
            <p className="text-[13px] text-text-primary font-medium">
              {t('scenes.noCustomScenes')}
            </p>
            <p className="text-[12px] text-text-secondary mt-1">{t('scenes.noCustomScenesDesc')}</p>
          </div>
        ) : (
          <div className="space-y-2">
            {config.custom_scenes.map((scene) => (
              <LocalSceneCard
                key={scene.id}
                id={scene.id}
                name={scene.name}
                description={scene.description}
                promptTemplate={scene.prompt_template}
                active={
                  config.active_scene?.source === 'custom' && config.active_scene.id === scene.id
                }
                copied={copiedId === scene.id}
                onActivate={() => handleActivateCustom(scene)}
                onCopy={() => handleCopyPrompt(scene.id, scene.prompt_template)}
                onEdit={() => handleStartEdit(scene)}
                onDuplicate={() => handleDuplicateCustom(scene)}
                onDelete={() => handleDeleteCustom(scene)}
              />
            ))}
          </div>
        )}
      </section>

      <section className="space-y-3">
        <h3 className="text-[13px] font-semibold text-text-primary">{t('scenes.builtInScenes')}</h3>
        <div className="space-y-2">
          {BUILTIN_SCENES.map((scene) => (
            <LocalSceneCard
              key={scene.id}
              id={scene.id}
              name={t(scene.nameKey)}
              description={t(scene.descriptionKey)}
              promptTemplate={scene.promptTemplate}
              active={
                config.active_scene?.source === 'builtin' && config.active_scene.id === scene.id
              }
              copied={copiedId === scene.id}
              onActivate={() => handleActivateBuiltIn(scene)}
              onCopy={() => handleCopyPrompt(scene.id, scene.promptTemplate)}
              onDuplicate={() => handleDuplicateBuiltIn(scene)}
            />
          ))}
        </div>
      </section>

      {mergeMsg && (
        <p className={`text-[12px] ${!mergeOk ? 'text-red-500' : 'text-green-500'}`}>{mergeMsg}</p>
      )}
      {saveError && <p className="text-[12px] text-red-500">{saveError}</p>}
    </div>
  )
}

function SceneEditor({
  editor,
  onChange,
  onCancel,
  onSave,
  onSaveAndActivate,
}: {
  editor: EditorState
  onChange: (editor: EditorState) => void
  onCancel: () => void
  onSave: () => void
  onSaveAndActivate: () => void
}) {
  const { t } = useTranslation()
  const canSave = editor.name.trim().length > 0 && editor.promptTemplate.trim().length > 0

  return (
    <div className="space-y-3 rounded-[8px] border border-border bg-bg-secondary px-3 py-3">
      <label className="block text-[12px] text-text-secondary">
        <span className="block mb-1">{t('scenes.sceneName')}</span>
        <input
          value={editor.name}
          onChange={(e) => onChange({ ...editor, name: e.target.value })}
          maxLength={80}
          className="w-full px-3 py-2 rounded-[8px] border border-border bg-bg-primary text-[13px] text-text-primary outline-none focus:border-border-focus"
        />
      </label>
      <label className="block text-[12px] text-text-secondary">
        <span className="block mb-1">{t('scenes.sceneDescription')}</span>
        <input
          value={editor.description}
          onChange={(e) => onChange({ ...editor, description: e.target.value })}
          maxLength={240}
          className="w-full px-3 py-2 rounded-[8px] border border-border bg-bg-primary text-[13px] text-text-primary outline-none focus:border-border-focus"
        />
      </label>
      <label className="block text-[12px] text-text-secondary">
        <span className="block mb-1">{t('scenes.promptTemplate')}</span>
        <textarea
          value={editor.promptTemplate}
          onChange={(e) => onChange({ ...editor, promptTemplate: e.target.value })}
          maxLength={4000}
          rows={5}
          className="w-full resize-y px-3 py-2 rounded-[8px] border border-border bg-bg-primary text-[13px] text-text-primary outline-none focus:border-border-focus"
        />
      </label>
      <div className="flex items-center justify-end gap-2">
        <button
          onClick={onCancel}
          className="px-3 py-1.5 rounded-[8px] border border-border bg-transparent text-[12px] text-text-secondary cursor-pointer hover:text-text-primary transition-colors"
        >
          {t('common.cancel')}
        </button>
        <button
          onClick={onSave}
          disabled={!canSave}
          className="px-3 py-1.5 rounded-[8px] border border-border bg-bg-primary text-[12px] text-text-primary cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {t('common.save')}
        </button>
        <button
          onClick={onSaveAndActivate}
          disabled={!canSave}
          className="px-3 py-1.5 rounded-[8px] border border-accent bg-accent text-white text-[12px] cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {t('scenes.saveAndActivate')}
        </button>
      </div>
    </div>
  )
}

function LocalSceneCard({
  id,
  name,
  description,
  promptTemplate,
  active,
  copied,
  onActivate,
  onCopy,
  onEdit,
  onDuplicate,
  onDelete,
}: {
  id: string
  name: string
  description: string
  promptTemplate: string
  active: boolean
  copied: boolean
  onActivate: () => void
  onCopy: () => void
  onEdit?: () => void
  onDuplicate: () => void
  onDelete?: () => void
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="border border-border rounded-[8px] overflow-hidden">
      <button
        onClick={() => setExpanded((value) => !value)}
        className="w-full flex items-center justify-between gap-3 px-3 py-2.5 bg-transparent border-none text-left cursor-pointer hover:bg-bg-secondary/50 transition-colors"
      >
        <span className="min-w-0">
          <span className="flex items-center gap-2">
            <span className="text-[13px] text-text-primary font-medium truncate">{name}</span>
            {active && (
              <span className="text-[10px] text-accent bg-accent/10 px-1.5 py-0.5 rounded-full">
                {t('scenes.active')}
              </span>
            )}
          </span>
          {description && (
            <span className="block text-[12px] text-text-tertiary truncate mt-0.5">
              {description}
            </span>
          )}
        </span>
        <span className="text-[11px] text-text-tertiary">{expanded ? '−' : '+'}</span>
      </button>
      {expanded && (
        <div className="border-t border-border px-3 py-3 space-y-3">
          <pre className="text-[12px] text-text-primary bg-bg-secondary rounded-[8px] px-3 py-2 whitespace-pre-wrap max-h-[140px] overflow-y-auto leading-relaxed">
            {promptTemplate}
          </pre>
          <div className="flex items-center gap-3 flex-wrap">
            {!active && (
              <button
                onClick={onActivate}
                className="text-[12px] text-accent bg-transparent border-none cursor-pointer hover:opacity-80"
              >
                {t('scenes.activate')}
              </button>
            )}
            <button
              onClick={onCopy}
              aria-label={`Copy prompt for ${name}`}
              className="flex items-center gap-1 text-[12px] text-accent bg-transparent border-none cursor-pointer hover:opacity-80"
            >
              {copied ? <Check size={12} /> : <Copy size={12} />}
              {copied ? t('scenes.copied') : t('scenes.copyPrompt')}
            </button>
            <button
              onClick={onDuplicate}
              className="text-[12px] text-accent bg-transparent border-none cursor-pointer hover:opacity-80"
            >
              {t('scenes.duplicate')}
            </button>
            {onEdit && (
              <button
                onClick={onEdit}
                className="flex items-center gap-1 text-[12px] text-accent bg-transparent border-none cursor-pointer hover:opacity-80"
              >
                <Pencil size={12} />
                {t('scenes.edit')}
              </button>
            )}
            {onDelete && (
              <button
                onClick={onDelete}
                className="flex items-center gap-1 text-[12px] text-red-500 bg-transparent border-none cursor-pointer hover:opacity-80"
              >
                <Trash2 size={12} />
                {t('scenes.delete')}
              </button>
            )}
          </div>
        </div>
      )}
      <span className="sr-only">{id}</span>
    </div>
  )
}
