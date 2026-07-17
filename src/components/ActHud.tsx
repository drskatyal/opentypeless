import { useCallback, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { Sparkles, ShieldAlert, MessageCircle, Loader2, X } from 'lucide-react'
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
  | { kind: 'step'; label: string }
  | { kind: 'confirm'; summary: string; reason: string }
  | { kind: 'ask_user'; prompt: string; options: AskOption[] }
  | { kind: 'say'; text: string }
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

// States during which the Act pill is shown. Includes the Conductor's own
// lifecycle names so the pill never vanishes mid-flow ("HUD dead but engine live").
const ACTIVE_STATES = new Set([
  'armed',
  'listening',
  'transcribing',
  'planning',
  'executing',
  'working',
  'awaiting_confirm',
  'awaiting_choice',
])

// A stale step label is cleared after this long with no new step, so the pill
// never reads "launching Spotify" forever if the backend goes quiet.
const STEP_STALE_MS = 25_000
// A talk-back answer persists this long (longer than a result toast) before
// auto-dismissing, unless the user dismisses it or a new command arrives.
const SAY_TIMEOUT_MS = 15_000
const STEP_MAX_CHARS = 44
const SAY_MAX_CHARS = 280

function truncate(text: string, max: number): string {
  const t = text.trim()
  return t.length > max ? `${t.slice(0, max - 1).trimEnd()}…` : t
}

export function ActHud() {
  const { t } = useTranslation()
  const [state, setState] = useState<string>('idle')
  const [prompt, setPrompt] = useState<Prompt>(null)
  const [step, setStep] = useState<string | null>(null)
  const [say, setSay] = useState<string | null>(null)
  const stepTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const sayTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const clearStep = useCallback(() => {
    if (stepTimer.current) clearTimeout(stepTimer.current)
    stepTimer.current = null
    setStep(null)
  }, [])

  const dismissSay = useCallback(() => {
    if (sayTimer.current) clearTimeout(sayTimer.current)
    sayTimer.current = null
    setSay(null)
  }, [])

  const abort = useCallback(() => {
    setPrompt(null)
    clearStep()
    actAbort().catch((err) => console.error('Failed to abort Act:', err))
  }, [clearStep])

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
          if (!ACTIVE_STATES.has(payload.state)) {
            setPrompt(null)
            clearStep()
          }
          break
        case 'step': {
          // Content follows the latest event; visibility is gated by state below.
          // Ignore blank labels (fall back to the generic "working" pill).
          const label = payload.label?.trim() ? truncate(payload.label, STEP_MAX_CHARS) : null
          setStep(label)
          if (stepTimer.current) clearTimeout(stepTimer.current)
          stepTimer.current = setTimeout(() => setStep(null), STEP_STALE_MS)
          break
        }
        case 'confirm':
          clearStep()
          dismissSay()
          setPrompt({ kind: 'confirm', summary: payload.summary, reason: payload.reason })
          break
        case 'ask_user':
          clearStep()
          dismissSay()
          setPrompt({ kind: 'ask_user', prompt: payload.prompt, options: payload.options })
          break
        case 'say':
          // A talk-back answer: show as a readable card, never a fleeting toast.
          // Suppressed while a decision modal is open (don't stack over a choice).
          setSay((prev) => {
            void prev
            return truncate(payload.text ?? '', SAY_MAX_CHARS) || null
          })
          if (sayTimer.current) clearTimeout(sayTimer.current)
          sayTimer.current = setTimeout(() => setSay(null), SAY_TIMEOUT_MS)
          break
        case 'result':
          setPrompt(null)
          clearStep()
          toast(payload.summary, payload.ok ? 'success' : 'error')
          break
        case 'error':
          setPrompt(null)
          clearStep()
          toast(payload.message, 'error')
          break
        default:
          // Unknown future variant: ignore, never throw.
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
      if (stepTimer.current) clearTimeout(stepTimer.current)
      if (sayTimer.current) clearTimeout(sayTimer.current)
    }
  }, [clearStep, dismissSay])

  // Keyboard: Esc dismisses the say card first, then aborts a prompt. Numbered
  // keys (1-9) pick an option while the choice modal is open.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (say && !prompt) {
          e.preventDefault()
          dismissSay()
          return
        }
        if (prompt) {
          e.preventDefault()
          abort()
        }
        return
      }
      if (prompt?.kind === 'ask_user' && /^[1-9]$/.test(e.key)) {
        const opt = prompt.options.find((o) => o.index === Number(e.key))
        if (opt) {
          e.preventDefault()
          decide('ask_user_pick', opt.index)
        }
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [prompt, say, abort, dismissSay, decide])

  const armed = ACTIVE_STATES.has(state)
  const working = state === 'working' || state === 'executing' || state === 'planning'
  const pillLabel = step ?? (working ? t('act.working') : t('act.armed'))

  return (
    <>
      {/* Talk-back answer card (above the pill, non-blocking) */}
      <AnimatePresence>
        {say && !prompt && (
          <motion.div
            key="act-say"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
            transition={{ type: 'spring', stiffness: 400, damping: 30 }}
            className="fixed bottom-16 left-4 z-[9998] flex max-w-[340px] items-start gap-2 rounded-[12px] border border-border bg-bg-primary px-3 py-2.5 shadow-float"
            role="status"
            aria-live="polite"
          >
            <MessageCircle size={15} className="mt-0.5 flex-shrink-0 text-accent" />
            <p className="min-w-0 flex-1 text-[13px] leading-relaxed text-text-primary">{say}</p>
            <button
              type="button"
              aria-label={t('act.dismiss')}
              onClick={dismissSay}
              className="-mr-1 -mt-0.5 flex-shrink-0 rounded-md p-1 text-text-tertiary hover:bg-bg-tertiary hover:text-text-primary transition-colors"
            >
              <X size={14} />
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Act pill: mode + live step. Text updates in place (no remount) to avoid flicker. */}
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
            aria-live="polite"
          >
            {working ? (
              <Loader2 size={13} className="flex-shrink-0 animate-spin text-accent" />
            ) : (
              <span className="relative flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
              </span>
            )}
            <span className="max-w-[240px] truncate text-[12px] font-medium text-text-primary">
              {pillLabel}
            </span>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Confirm / ask_user modal (centered, blocking) */}
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
              role={prompt.kind === 'confirm' ? 'alertdialog' : 'dialog'}
              aria-modal="true"
              aria-labelledby="act-prompt-title"
            >
              {prompt.kind === 'confirm' ? (
                <>
                  <div className="mb-2 flex items-center gap-2">
                    <ShieldAlert size={16} className="flex-shrink-0 text-warning" />
                    <h3 id="act-prompt-title" className="text-[14px] font-medium text-text-primary">
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
                      autoFocus
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
                    <h3 id="act-prompt-title" className="text-[14px] font-medium text-text-primary">
                      {prompt.prompt}
                    </h3>
                  </div>
                  <div className="mt-2 flex flex-col gap-1.5">
                    {prompt.options.map((opt, i) => (
                      <button
                        key={opt.index}
                        type="button"
                        autoFocus={i === 0}
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
