import { useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '../stores/appStore'

export function useRecording() {
  const { pipelineState, resetRecording } = useAppStore()

  const startRecording = useCallback(async () => {
    resetRecording()
    await invoke('start_recording')
  }, [resetRecording])

  const stopRecording = useCallback(async () => {
    await invoke('stop_recording')
  }, [])

  const isPreparing = pipelineState === 'preparing'
  const isRecording = pipelineState === 'recording'
  const isProcessing =
    pipelineState === 'preparing' ||
    pipelineState === 'transcribing' ||
    pipelineState === 'polishing'
  const isIdle = pipelineState === 'idle'

  return {
    startRecording,
    stopRecording,
    isPreparing,
    isRecording,
    isProcessing,
    isIdle,
    pipelineState,
  }
}
