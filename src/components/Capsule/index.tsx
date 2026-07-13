import { useRef, useCallback, useEffect } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'
import { useRecording } from '../../hooks/useRecording'
import { useCapsuleResize } from '../../hooks/useCapsuleResize'
import { stopAskFlow } from '../../lib/tauri'
import { CapsuleIdle } from './CapsuleIdle'
import { CapsulePreparing } from './CapsulePreparing'
import { CapsuleRecording } from './CapsuleRecording'
import { CapsuleProcessing } from './CapsuleProcessing'
import { CapsulePolishing } from './CapsulePolishing'
import { CapsuleComplete } from './CapsuleComplete'
import { CapsuleError } from './CapsuleError'
import { CapsuleContextMenu } from './CapsuleContextMenu'
import { CapsuleAskRecording } from './CapsuleAskRecording'
import { CapsuleAskThinking } from './CapsuleAskThinking'
import { TranslateTargetMenu } from './TranslateTargetChip'

const DRAG_THRESHOLD = 5

function getCapsuleState(pipelineState: string, hasError: boolean) {
  if (hasError) return 'error'
  return pipelineState
}

function getCapsuleShellSize(capsuleState: string) {
  switch (capsuleState) {
    case 'idle':
      return { width: 36, height: 36 }
    case 'preparing':
      return { width: 180, height: 36 }
    case 'outputting':
      return { width: 144, height: 36 }
    case 'ask_recording':
    case 'ask_thinking':
      return { width: 168, height: 36 }
    case 'recording':
    case 'transcribing':
    case 'polishing':
    case 'error':
      return { width: 200, height: 36 }
    default:
      return { width: 36, height: 36 }
  }
}

export function Capsule() {
  const pipelineState = useAppStore((s) => s.pipelineState)
  const pipelineError = useAppStore((s) => s.pipelineError)
  const contextMenuOpen = useAppStore((s) => s.contextMenuOpen)
  const setContextMenuOpen = useAppStore((s) => s.setContextMenuOpen)
  const contextMenuReady = useAppStore((s) => s.contextMenuReady)
  const setContextMenuReady = useAppStore((s) => s.setContextMenuReady)
  const translationTargetMenuOpen = useAppStore((s) => s.translationTargetMenuOpen)
  const setTranslationTargetMenuOpen = useAppStore((s) => s.setTranslationTargetMenuOpen)
  const { stopRecording, isRecording } = useRecording()

  const dragStart = useRef<{ x: number; y: number } | null>(null)
  const isDragging = useRef(false)

  useCapsuleResize()

  const hasError = pipelineError !== null
  const capsuleState = getCapsuleState(pipelineState, hasError)
  const capsuleShellSize = getCapsuleShellSize(capsuleState)

  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return
    dragStart.current = { x: e.clientX, y: e.clientY }
    isDragging.current = false
  }, [])

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragStart.current || isDragging.current) return
    const dx = e.clientX - dragStart.current.x
    const dy = e.clientY - dragStart.current.y
    if (Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD) {
      isDragging.current = true
      dragStart.current = null
      import('@tauri-apps/api/window')
        .then(({ getCurrentWindow }) => {
          getCurrentWindow()
            .startDragging()
            .catch(() => {})
        })
        .catch(() => {})
    }
  }, [])

  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0) return
      if (isDragging.current) {
        isDragging.current = false
        dragStart.current = null
        return
      }
      dragStart.current = null

      if (pipelineState === 'ask_recording') {
        void stopAskFlow().catch((error) => {
          console.error('Failed to stop Ask flow:', error)
        })
      } else if (isRecording) {
        stopRecording()
      }
    },
    [isRecording, pipelineState, stopRecording],
  )

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    if (!contextMenuOpen) {
      setTranslationTargetMenuOpen(false)
      setContextMenuOpen(true)
    }
  }

  const handleCloseMenu = () => {
    setContextMenuReady(false)
    setContextMenuOpen(false)
  }

  useEffect(() => {
    if (
      translationTargetMenuOpen &&
      (pipelineState !== 'recording' || capsuleState !== 'recording')
    ) {
      setTranslationTargetMenuOpen(false)
    }
  }, [capsuleState, pipelineState, setTranslationTargetMenuOpen, translationTargetMenuOpen])

  return (
    <div
      className="w-full h-full flex items-center justify-start relative"
      style={{ background: 'transparent' }}
      onContextMenu={handleContextMenu}
    >
      {/* Persistent outer shell — jelly capsule */}
      <motion.div
        layout
        transition={{ layout: { duration: 0.2, ease: [0.2, 0, 0, 1] } }}
        className={`absolute left-3 rounded-full pointer-events-auto shrink-0 ${
          capsuleState === 'error'
            ? 'jelly-capsule-error'
            : capsuleState === 'idle'
              ? 'jelly-capsule text-neutral-700'
              : 'jelly-capsule-active text-white'
        }`}
        style={capsuleShellSize}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        <AnimatePresence mode="sync" initial={false}>
          <motion.div
            key={capsuleState}
            className="absolute inset-0"
            initial={{ opacity: 0, filter: 'blur(2px)' }}
            animate={{ opacity: 1, filter: 'blur(0px)' }}
            exit={{ opacity: 0, filter: 'blur(2px)' }}
            transition={{ duration: 0.12, ease: [0.2, 0, 0, 1] }}
          >
            {capsuleState === 'idle' && <CapsuleIdle />}
            {capsuleState === 'preparing' && <CapsulePreparing />}
            {capsuleState === 'recording' && <CapsuleRecording />}
            {capsuleState === 'transcribing' && <CapsuleProcessing />}
            {capsuleState === 'polishing' && <CapsulePolishing />}
            {capsuleState === 'outputting' && <CapsuleComplete />}
            {capsuleState === 'ask_recording' && <CapsuleAskRecording />}
            {capsuleState === 'ask_thinking' && <CapsuleAskThinking />}
            {capsuleState === 'error' && <CapsuleError />}
          </motion.div>
        </AnimatePresence>
      </motion.div>

      {/* Context menu appears to the right of capsule */}
      {contextMenuOpen && contextMenuReady && (
        <div className="ml-2">
          <CapsuleContextMenu onClose={handleCloseMenu} />
        </div>
      )}
      <TranslateTargetMenu />
    </div>
  )
}
