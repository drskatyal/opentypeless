import { useCallback, useRef, useState } from 'react'
import { Bold, Italic, Underline, List, ListOrdered, Heading1, X } from 'lucide-react'
import { getCurrentWindow } from '@tauri-apps/api/window'

type ExecCommand =
  | 'bold'
  | 'italic'
  | 'underline'
  | 'insertUnorderedList'
  | 'insertOrderedList'
  | 'formatBlock'

interface ToolbarButton {
  label: string
  command: ExecCommand
  value?: string
  icon: typeof Bold
}

const TOOLBAR_BUTTONS: ToolbarButton[] = [
  { label: 'Bold', command: 'bold', icon: Bold },
  { label: 'Italic', command: 'italic', icon: Italic },
  { label: 'Underline', command: 'underline', icon: Underline },
  { label: 'Bulleted list', command: 'insertUnorderedList', icon: List },
  { label: 'Numbered list', command: 'insertOrderedList', icon: ListOrdered },
  { label: 'Heading', command: 'formatBlock', value: 'h1', icon: Heading1 },
]

function currentNativeWindow() {
  try {
    return getCurrentWindow()
  } catch {
    return null
  }
}

export function FloatingEditor() {
  const editorRef = useRef<HTMLDivElement>(null)
  const [isEmpty, setIsEmpty] = useState(true)

  const focusEditor = useCallback(() => {
    editorRef.current?.focus()
  }, [])

  const runCommand = useCallback(
    (button: ToolbarButton) => {
      focusEditor()
      // execCommand is deprecated but remains the simplest first-pass rich-text
      // approach and is broadly supported inside the Tauri webview.
      document.execCommand(button.command, false, button.value)
      setIsEmpty(!editorRef.current?.textContent?.trim())
    },
    [focusEditor],
  )

  const handleInput = useCallback(() => {
    setIsEmpty(!editorRef.current?.textContent?.trim())
  }, [])

  const hideWindow = useCallback(() => {
    const win = currentNativeWindow()
    void win?.hide().catch(() => {})
  }, [])

  const startDrag = useCallback((event: React.MouseEvent<HTMLElement>) => {
    if ((event.target as HTMLElement).closest('button')) return
    const win = currentNativeWindow()
    void win?.startDragging().catch(() => {})
  }, [])

  // Dictation is verbatim — pasting must not smuggle in rich HTML/markup that
  // could later alter the serialized text. Force plain text on paste.
  const handlePaste = useCallback((event: React.ClipboardEvent<HTMLDivElement>) => {
    event.preventDefault()
    const text = event.clipboardData.getData('text/plain')
    document.execCommand('insertText', false, text)
    setIsEmpty(!editorRef.current?.textContent?.trim())
  }, [])

  return (
    <div className="h-screen w-screen bg-transparent p-4 text-text-primary">
      <section className="flex h-full w-full flex-col overflow-hidden rounded-[16px] border border-border bg-bg-primary shadow-[0_4px_12px_rgba(0,0,0,0.22)]">
        {/* Draggable title bar */}
        <div
          onMouseDown={startDrag}
          className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-3 py-2"
        >
          <div className="flex items-center gap-2">
            <span className="h-2 w-2 rounded-full bg-accent" />
            <span className="text-[12px] font-medium text-text-primary">Editor</span>
          </div>
          <button
            type="button"
            aria-label="Close editor"
            title="Close"
            onClick={hideWindow}
            className="flex h-7 w-7 items-center justify-center rounded-[6px] border border-border bg-bg-secondary text-text-tertiary transition-colors hover:border-border-focus hover:text-accent cursor-pointer"
          >
            <X size={13} />
          </button>
        </div>

        {/* Formatting toolbar */}
        <div className="flex shrink-0 flex-wrap items-center gap-1 border-b border-border px-2 py-1.5">
          {TOOLBAR_BUTTONS.map((button) => {
            const Icon = button.icon
            return (
              <button
                key={button.label}
                type="button"
                aria-label={button.label}
                title={button.label}
                // Prevent the button from stealing the editor selection on mousedown.
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => runCommand(button)}
                className="flex h-8 w-8 items-center justify-center rounded-[6px] text-text-secondary transition-colors hover:bg-bg-secondary hover:text-accent cursor-pointer"
              >
                <Icon size={15} />
              </button>
            )
          })}
        </div>

        {/* Rich-text editable area */}
        <div className="relative min-h-0 flex-1 overflow-y-auto">
          {isEmpty && (
            <p className="pointer-events-none absolute left-3 top-3 text-[13px] text-text-tertiary">
              Start writing…
            </p>
          )}
          <div
            ref={editorRef}
            role="textbox"
            aria-label="Rich text editor"
            aria-multiline="true"
            contentEditable
            suppressContentEditableWarning
            onInput={handleInput}
            onPaste={handlePaste}
            className="h-full w-full px-3 py-3 text-[13px] leading-6 text-text-primary outline-none [&_h1]:mb-1 [&_h1]:text-[18px] [&_h1]:font-semibold [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
          />
        </div>
      </section>
    </div>
  )
}
