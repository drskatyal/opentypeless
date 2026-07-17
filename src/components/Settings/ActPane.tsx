import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Search } from 'lucide-react'
import { actListFlows } from '../../lib/tauri'
import type { ActFlowInfo } from '../../lib/tauri'

/**
 * A dedicated Settings pane listing the built-in Act drawer recipes — the full,
 * searchable catalog of everything the user can say when Act is armed. Loads the
 * (static) list once on mount.
 */
export function ActPane() {
  const { t } = useTranslation()
  const [flows, setFlows] = useState<ActFlowInfo[] | null>(null)
  const [error, setError] = useState(false)
  const [query, setQuery] = useState('')

  const load = useCallback(() => {
    setError(false)
    actListFlows()
      .then(setFlows)
      .catch((err) => {
        console.error('Failed to load Act flow list:', err)
        setError(true)
      })
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const filtered = useMemo(() => {
    if (!flows) return []
    const q = query.trim().toLowerCase()
    if (!q) return flows
    return flows.filter((f) => {
      const haystack = [f.name, f.description, ...f.aliases, ...f.slots].join(' ').toLowerCase()
      return haystack.includes(q)
    })
  }, [flows, query])

  return (
    <div className="space-y-4">
      <p className="text-[12px] leading-relaxed text-text-tertiary">
        {t('settings.actFlowsIntro')}
      </p>

      {error ? (
        <p className="text-[12px] text-warning">{t('settings.actFlowsLoadError')}</p>
      ) : flows === null ? (
        <p className="text-[12px] text-text-tertiary">…</p>
      ) : (
        <>
          <div className="relative">
            <Search
              size={14}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text-tertiary"
            />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('settings.actFlowsSearch')}
              className="w-full rounded-[8px] border border-border bg-bg-primary py-2 pl-9 pr-3 text-[13px] text-text-primary outline-none focus:border-border-focus"
            />
          </div>

          <p className="text-[11px] text-text-tertiary">
            {t('settings.actFlowsCount', { count: flows.length })}
          </p>

          {filtered.length === 0 ? (
            <p className="text-[12px] text-text-tertiary">{t('settings.actFlowsEmpty')}</p>
          ) : (
            <ul className="space-y-2">
              {filtered.map((flow) => (
                <FlowRow key={flow.id} flow={flow} />
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  )
}

function FlowRow({ flow }: { flow: ActFlowInfo }) {
  const { t } = useTranslation()
  // Aliases carry `{slot}` placeholders (e.g. "open {app}"); drop the braces so
  // the example reads naturally ("open app").
  const example = flow.aliases[0]?.replace(/[{}]/g, '')

  return (
    <li className="rounded-[10px] border border-border bg-bg-secondary/40 px-3 py-2.5">
      <div className="flex items-start justify-between gap-2">
        <p className="text-[13px] font-medium text-text-primary">{flow.name}</p>
        {flow.kind === 'branch' && (
          <span className="shrink-0 rounded-full border border-border bg-bg-tertiary px-2 py-0.5 text-[10px] font-medium text-text-secondary">
            {t('settings.actFlowsBranch')}
          </span>
        )}
      </div>
      {flow.description && (
        <p className="mt-0.5 text-[12px] leading-relaxed text-text-tertiary">{flow.description}</p>
      )}
      {example && (
        <p className="mt-1.5 text-[12px] text-text-secondary">
          <span className="text-text-tertiary">{t('settings.actFlowsExample')}: </span>
          <span className="italic">“{example}”</span>
        </p>
      )}
      {flow.slots.length > 0 && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          <span className="text-[11px] text-text-tertiary">{t('settings.actFlowsFills')}:</span>
          {flow.slots.map((slot) => (
            <span
              key={slot}
              className="rounded-[6px] bg-bg-tertiary px-1.5 py-0.5 font-mono text-[10px] text-text-secondary"
            >
              {slot}
            </span>
          ))}
        </div>
      )}
    </li>
  )
}
