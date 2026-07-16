import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { Sparkles, ShieldAlert } from 'lucide-react'
import { actAbort, actUserDecision } from '../lib/tauri'
import { toast } from './toast-service'

type Unlisten = () => void | Promise<void>

interface AskOption {
  index: number
  label: string
  path: string
}

type ActEvent =
  | { kind: 'state'; state: string }
  | { kind: 'confirm'; summary: string; reason: string }
  | { kind: 'ask_user'; prompt: string; options: AskOption[] }
  | { kind: 'result'; ok: boolean; summary: string }
  | { kind: 'error'; message: string }

type Prompt =
  | { kind: 'confirm'; summary: string; reason: string }
  | { kind: 'ask_user'; prompt: string; options: AskOption[] }
  | null

function safeUnlisten(unlisten: Unlisten) {
  try {
    Promise.resolve(unlisten()).catch(() => {})
  } catch {
    // Dev HMR can leave Tauri listener handles stale.
  }
}

const ACTIVE_STATES = new Set(['armed', 'listening', 'transcribing', 'planning', 'executing'])

export function ActHud() {
  const { t } = useTranslation()
  const [state, setState] = useState<string>('idle')
  const [prompt, setPrompt] = useState<Prompt>(null)

  const abort = useCallback(() => {
    setPrompt(null)
    actAbort().catch((err) => console.error('Failed to abort Act:', err))
  }, [])

  const decide = useCallback((decision: string, index?: number) => {
    setPrompt(null)
    actUserDecision(decision, index).catch((err) =>
      console.error('Failed to send Act decision:', err),
    )
  }, [])

  useEffect(() => {
    let cancelled = false
    let unlisten: Unlisten | null = null

    listen<ActEvent>('act://event', (event) => {
      const payload = event.payload
      switch (payload.kind) {
        case 'state':
          setState(payload.state)
          if (!ACTIVE_STATES.has(payload.state)) setPrompt(null)
          break
        case 'confirm':
          setPrompt({ kind: 'confirm', summary: payload.summary, reason: payload.reason })
          break
        case 'ask_user':
          setPrompt({ kind: 'ask_user', prompt: payload.prompt, options: payload.options })
          break
        case 'result':
          setPrompt(null)
          toast(payload.summary, payload.ok ? 'success' : 'error')
          break
        case 'error':
          setPrompt(null)
          toast(payload.message, 'error')
          break
      }
    })
      .then((fn) => {
        if (cancelled) safeUnlisten(fn)
        else unlisten = fn
      })
      .catch((err) => console.error('Failed to register Act listener:', err))

    return () => {
      cancelled = true
      if (unlisten) safeUnlisten(unlisten)
    }
  }, [])

  // Esc cancels/aborts while a prompt is showing.
  useEffect(() => {
    if (!prompt) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      abort()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [prompt, abort])

  const armed = ACTIVE_STATES.has(state)

  return (
    <>
      {/* Subtle armed / state indicator */}
      <AnimatePresence>
        {armed && !prompt && (
          <motion.div
            key="act-indicator"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            className="fixed bottom-4 left-4 z-[9998] flex items-center gap-2 rounded-full border border-accent/30 bg-bg-secondary px-3 py-1.5 shadow-lg"
            role="status"
          >
            <span className="relative flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
            </span>
            <span className="text-[12px] font-medium text-text-primary">{t('act.armed')}</span>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Confirm / ask_user modal */}
      <AnimatePresence>
        {prompt && (
          <motion.div
            key="act-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.12 }}
            className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/30 px-4"
          >
            <motion.div
              initial={{ opacity: 0, scale: 0.96, y: 8 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.96, y: 8 }}
              transition={{ type: 'spring', stiffness: 400, damping: 28 }}
              className="w-full max-w-[380px] rounded-[14px] border border-border bg-bg-primary p-4 shadow-float"
              role="dialog"
              aria-modal="true"
            >
              {prompt.kind === 'confirm' ? (
                <>
                  <div className="mb-2 flex items-center gap-2">
                    <ShieldAlert size={16} className="flex-shrink-0 text-warning" />
                    <h3 className="text-[14px] font-medium text-text-primary">
                      {t('act.confirmTitle')}
                    </h3>
                  </div>
                  <p className="text-[13px] leading-relaxed text-text-primary">{prompt.summary}</p>
                  {prompt.reason && (
                    <p className="mt-1.5 text-[12px] leading-relaxed text-text-tertiary">
                      {prompt.reason}
                    </p>
                  )}
                  <div className="mt-4 flex justify-end gap-2">
                    <button
                      type="button"
                      onClick={() => decide('confirm_deny')}
                      className="rounded-[10px] border border-border bg-transparent px-3 py-1.5 text-[13px] text-text-secondary hover:bg-bg-tertiary hover:text-text-primary transition-colors"
                    >
                      {t('act.deny')}
                    </button>
                    <button
                      type="button"
                      onClick={() => decide('confirm_allow')}
                      className="rounded-[10px] border-none bg-accent px-3 py-1.5 text-[13px] text-white hover:bg-accent-hover transition-colors"
                    >
                      {t('act.allow')}
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div className="mb-2 flex items-center gap-2">
                    <Sparkles size={16} className="flex-shrink-0 text-accent" />
                    <h3 className="text-[14px] font-medium text-text-primary">{prompt.prompt}</h3>
                  </div>
                  <div className="mt-2 flex flex-col gap-1.5">
                    {prompt.options.map((opt) => (
                      <button
                        key={opt.index}
                        type="button"
                        onClick={() => decide('ask_user_pick', opt.index)}
                        className="flex items-center gap-2.5 rounded-[10px] border border-border bg-bg-secondary px-3 py-2 text-left text-[13px] text-text-primary hover:border-border-focus transition-colors"
                      >
                        <span className="grid h-5 w-5 flex-shrink-0 place-items-center rounded-full bg-bg-tertiary text-[11px] font-medium text-text-secondary">
                          {opt.index}
                        </span>
                        <span className="min-w-0 truncate">{opt.label}</span>
                      </button>
                    ))}
                  </div>
                  <div className="mt-4 flex justify-end">
                    <button
                      type="button"
                      onClick={abort}
                      className="rounded-[10px] border border-border bg-transparent px-3 py-1.5 text-[13px] text-text-secondary hover:bg-bg-tertiary hover:text-text-primary transition-colors"
                    >
                      {t('act.cancel')}
                    </button>
                  </div>
                </>
              )}
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  )
}
