import { ChevronDown } from 'lucide-react'
import type { SelectHTMLAttributes } from 'react'

interface Props extends SelectHTMLAttributes<HTMLSelectElement> {
  /** Stretch to fill the available width (default: intrinsic min-width). */
  fluid?: boolean
}

/**
 * Claude-Desktop-style dropdown. A native `<select>` (so all existing
 * value/onChange/aria wiring keeps working and it stays a `combobox` role)
 * restyled to match the mockup, with a custom chevron affordance.
 */
export function Select({ fluid = false, className = '', children, ...rest }: Props) {
  return (
    <div className={`relative ${fluid ? 'w-full' : ''}`}>
      <select
        {...rest}
        className={`w-full appearance-none rounded-sm border border-border bg-bg-secondary py-2 pl-3 pr-9 text-[13px] text-text-primary outline-none transition-colors focus:border-border-focus disabled:cursor-not-allowed disabled:opacity-50 ${
          fluid ? '' : 'min-w-[168px]'
        } ${className}`}
      >
        {children}
      </select>
      <ChevronDown
        size={15}
        aria-hidden="true"
        className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-text-tertiary"
      />
    </div>
  )
}
