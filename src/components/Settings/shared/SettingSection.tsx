interface Props {
  /** Small uppercase group heading (mockup `.section__hd h2`). Optional. */
  title?: string
  /** Optional one-line group description under the heading. */
  description?: string
  children: React.ReactNode
  className?: string
}

/**
 * Claude-Desktop-style titled group card. Wraps a set of {@link SettingRow}s (or
 * any settings markup) in an elevated, hairline-bordered card. Purely
 * presentational — no bindings.
 */
export function SettingSection({ title, description, children, className = '' }: Props) {
  return (
    <section
      className={`overflow-hidden rounded-md border border-border bg-bg-elevated shadow-sm ${className}`}
    >
      {(title || description) && (
        <div className="px-[18px] pb-2.5 pt-3.5">
          {title && (
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-text-tertiary">
              {title}
            </h3>
          )}
          {description && <p className="mt-1 text-[12.5px] text-text-secondary">{description}</p>}
        </div>
      )}
      {children}
    </section>
  )
}
