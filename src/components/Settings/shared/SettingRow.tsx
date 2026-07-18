interface Props {
  /** Row label (left column). */
  label: React.ReactNode
  /** Optional secondary description under the label. */
  description?: React.ReactNode
  /** The control(s) shown on the right. */
  children?: React.ReactNode
  /** Vertical alignment of the control against the text block. */
  align?: 'center' | 'start'
  /**
   * Force the control to sit full-width BELOW the label instead of to the right.
   * Use for wide controls (segmented pickers, sliders, textareas).
   */
  stackControl?: boolean
  className?: string
}

/**
 * Claude-Desktop-style settings row: label + optional description on the left,
 * a right-aligned control slot on the right. Rows draw a hairline top border and
 * drop it for the first row in a group (`first:border-t-0`). At ≤520px the row
 * stacks vertically. Purely presentational — bindings live in the control passed
 * as `children`.
 */
export function SettingRow({
  label,
  description,
  children,
  align = 'center',
  stackControl = false,
  className = '',
}: Props) {
  return (
    <div
      className={`flex gap-5 border-t border-border px-[18px] py-3.5 first:border-t-0 max-[520px]:flex-col max-[520px]:items-stretch max-[520px]:gap-2.5 ${
        stackControl ? 'flex-col items-stretch' : align === 'start' ? 'items-start' : 'items-center'
      } ${className}`}
    >
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium text-text-primary">{label}</div>
        {description && (
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-text-secondary">
            {description}
          </div>
        )}
      </div>
      {children && (
        <div
          className={`flex flex-none items-center gap-2.5 max-[520px]:w-full max-[520px]:flex-wrap max-[520px]:justify-start ${
            stackControl ? 'w-full' : ''
          }`}
        >
          {children}
        </div>
      )}
    </div>
  )
}
