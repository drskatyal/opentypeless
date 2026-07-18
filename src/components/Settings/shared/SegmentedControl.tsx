import { useId } from 'react'
import { motion } from 'framer-motion'

interface Props {
  options: { value: string; label: string }[]
  value: string
  onChange: (value: string) => void
}

export function SegmentedControl({ options, value, onChange }: Props) {
  const id = useId()

  return (
    <div className="flex gap-0.5 rounded-[9px] border border-border bg-bg-tertiary/60 p-[3px]">
      {options.map((opt) => (
        <button
          key={opt.value}
          onClick={() => onChange(opt.value)}
          aria-pressed={value === opt.value}
          className={`relative flex-1 cursor-pointer rounded-[6px] border-none bg-transparent px-3 py-1.5 text-[12.5px] font-medium transition-colors ${
            value === opt.value
              ? 'text-text-primary'
              : 'text-text-secondary hover:text-text-primary'
          }`}
        >
          {value === opt.value && (
            <motion.div
              layoutId={`segment-bg-${id}`}
              className="absolute inset-0 rounded-[6px] border border-border bg-bg-elevated shadow-sm"
              transition={{ type: 'spring', stiffness: 400, damping: 18 }}
            />
          )}
          <span className="relative z-10">{opt.label}</span>
        </button>
      ))}
    </div>
  )
}
