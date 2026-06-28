import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Cloud, CreditCard, Crown, KeyRound, Loader2, Sparkles } from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { hasManagedCloudAccess, useAuthStore } from '../../stores/authStore'
import { CHECKOUT_PLANS, PRO_PLAN, type CheckoutProduct } from '../../lib/constants'
import { createCheckout } from '../../lib/api'

const benefitIcons = [Sparkles, KeyRound, Cloud]

export function UpgradePage() {
  const {
    user,
    plan,
    source,
    displayName,
    cloudWordsUsed,
    cloudWordsLimit,
    sttSecondsUsed,
    sttSecondsLimit,
    llmTokensUsed,
    llmTokensLimit,
  } = useAuthStore()
  const { t } = useTranslation()
  const [loadingProduct, setLoadingProduct] = useState<CheckoutProduct | null>(null)
  const [error, setError] = useState<string | null>(null)

  const hasCloudAccess = useAuthStore(hasManagedCloudAccess)
  const hasLifetimeAccess = plan === 'lifetime_starter' || source === 'lifetime' || source === 'appsumo'
  const hasMonthlyAccess = !hasLifetimeAccess && (plan === 'pro' || source === 'creem')
  const hasLifetimeCheckoutPlan = CHECKOUT_PLANS.some((checkoutPlan) => checkoutPlan.product === 'lifetime_starter')
  const visiblePlans = hasMonthlyAccess
    ? CHECKOUT_PLANS.filter((checkoutPlan) => checkoutPlan.product === 'lifetime_starter')
    : CHECKOUT_PLANS
  const canStartCheckout = (product: CheckoutProduct) => {
    if (hasLifetimeAccess) return false
    if (product === 'lifetime_starter') return true
    return !hasCloudAccess
  }

  const handleSubscribe = async (product: CheckoutProduct) => {
    setLoadingProduct(product)
    setError(null)
    try {
      const { url } = await createCheckout('desktop', product)
      useAuthStore.setState({ checkoutPending: true })
      await openUrl(url)
    } catch (e) {
      setError(e instanceof Error ? e.message : t('account.toast.subscriptionFail'))
    } finally {
      setLoadingProduct(null)
    }
  }

  return (
    <div className="max-w-[640px] mx-auto py-8 px-6 text-[13px]">
      {/* Header */}
      <div className="text-center mb-6">
        <div className="inline-flex items-center gap-2 mb-2">
          <Crown size={20} className="text-amber-500" />
          <h1 className="text-[20px] font-semibold text-text-primary">{t('upgrade.title')}</h1>
        </div>
        <p className="text-text-secondary">{t('upgrade.subtitle')}</p>
      </div>

      {/* Current plan badge */}
      <div className="flex items-center justify-center mb-6">
        <span
          className={`px-3 py-1 rounded-full text-[12px] font-medium ${
            hasCloudAccess
              ? 'bg-amber-500/10 text-amber-600'
              : 'bg-bg-secondary text-text-secondary'
          }`}
        >
          {t('upgrade.currentPlan', { plan: displayName })}
        </span>
      </div>

      {/* Pricing cards */}
      {visiblePlans.length > 0 && (
        <div className={`grid gap-3 mb-5 ${hasMonthlyAccess ? '' : 'min-[620px]:grid-cols-2'}`}>
          {visiblePlans.map((checkoutPlan) => {
            const isLoading = loadingProduct === checkoutPlan.product
            const isLifetime = checkoutPlan.product === 'lifetime_starter'
            const price = hasMonthlyAccess && isLifetime && checkoutPlan.upgradePrice
              ? checkoutPlan.upgradePrice
              : checkoutPlan.price
            const sublineKey = hasMonthlyAccess && isLifetime && checkoutPlan.upgradeSublineKey
              ? checkoutPlan.upgradeSublineKey
              : checkoutPlan.sublineKey
            return (
              <section
                key={checkoutPlan.product}
                className={`relative rounded-[10px] overflow-hidden ${
                  isLifetime
                    ? 'border border-amber-400/70 bg-amber-500/[0.06] shadow-[0_14px_40px_rgba(245,158,11,0.12)]'
                    : 'border border-border'
                }`}
              >
                <div
                  className={`px-4 py-4 border-b ${
                    isLifetime
                      ? 'border-amber-400/30 bg-amber-500/[0.08]'
                      : 'border-border bg-bg-secondary/50'
                  }`}
                >
                  <div className="flex items-start justify-between gap-2">
                    <h2 className="text-[14px] font-semibold text-text-primary">{t(checkoutPlan.nameKey)}</h2>
                    {checkoutPlan.badgeKey && (
                      <span className="shrink-0 rounded-full bg-amber-500 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.04em] text-white">
                        {t(checkoutPlan.badgeKey)}
                      </span>
                    )}
                  </div>
                  <p className="mt-2 text-[22px] font-semibold text-text-primary">
                    {price}
                    <span className="text-[13px] font-normal text-text-secondary">
                      {' '}
                      / {t(checkoutPlan.periodKey)}
                    </span>
                  </p>
                  <p className="mt-2 min-h-[36px] text-[12px] leading-5 text-text-secondary">
                    {t(checkoutPlan.descriptionKey)}
                  </p>
                  {sublineKey && (
                    <p className="mt-2 inline-flex items-center gap-1.5 rounded-full bg-amber-500/10 px-2 py-1 text-[11px] font-medium text-amber-700">
                      <Sparkles size={12} />
                      {t(sublineKey)}
                    </p>
                  )}
                </div>
                {canStartCheckout(checkoutPlan.product) && (
                  <div className="p-4">
                    <button
                      onClick={() => handleSubscribe(checkoutPlan.product)}
                      disabled={loadingProduct !== null || !user}
                      className={`w-full py-2.5 rounded-[8px] text-white text-[13px] font-medium cursor-pointer border-none hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 ${
                        isLifetime ? 'bg-amber-500' : 'bg-accent'
                      }`}
                    >
                      {isLoading ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : (
                        <CreditCard size={14} />
                      )}
                      {t(checkoutPlan.ctaKey)}
                    </button>
                  </div>
                )}
              </section>
            )
          })}
        </div>
      )}

      {/* Cloud plan benefits */}
      <div className="border border-border rounded-[10px] overflow-hidden mb-5">
        <section className="px-4 py-3 border-b border-border bg-bg-primary/40">
          <h2 className="text-[12px] font-semibold text-text-primary">
            {t('upgrade.benefits.title')}
          </h2>
          <div className="mt-3 grid gap-2">
            {PRO_PLAN.benefits.map((benefit, i) => {
              const Icon = benefitIcons[i] ?? Check
              return (
                <div key={benefit.labelKey} className="flex items-start gap-2.5">
                  <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-amber-500/10 text-amber-600">
                    <Icon size={12} />
                  </span>
                  <span className="text-[13px] leading-5 text-text-primary">
                    {t(benefit.labelKey)}
                  </span>
                </div>
              )
            })}
          </div>
        </section>
      </div>

      {/* Pro quota progress */}
      {hasCloudAccess && (
        <div className="border border-border rounded-[10px] overflow-hidden mb-5">
          <div className="px-4 py-2.5 bg-bg-secondary/50 border-b border-border">
            <h3 className="text-[13px] font-medium text-text-primary">
              {t('upgrade.usageThisMonth')}
            </h3>
          </div>
          <div className="px-4 py-3 space-y-3">
            {cloudWordsLimit > 0 ? (
              <QuotaBar
                label={t('account.cloudWords', 'Cloud words')}
                used={cloudWordsUsed}
                limit={cloudWordsLimit}
                unit={t('account.quotaKWords', 'k words')}
                divisor={1000}
              />
            ) : (
              <>
                <QuotaBar
                  label={t('upgrade.stt')}
                  used={sttSecondsUsed}
                  limit={sttSecondsLimit}
                  unit={t('account.quotaHours')}
                  divisor={3600}
                />
                <QuotaBar
                  label={t('upgrade.llm')}
                  used={llmTokensUsed}
                  limit={llmTokensLimit}
                  unit={t('account.quotaTokens')}
                  divisor={1000}
                />
              </>
            )}
          </div>
        </div>
      )}

      {/* Action */}
      {hasLifetimeAccess ? (
        <div className="text-center py-3">
          <p className="text-text-secondary flex items-center justify-center gap-1.5">
            <Crown size={14} className="text-amber-500" />
            {t('upgrade.thankYou')}
          </p>
        </div>
      ) : hasCloudAccess ? (
        <div className="text-center py-3">
          <p className="text-text-secondary flex items-center justify-center gap-1.5">
            <Crown size={14} className="text-amber-500" />
            {hasLifetimeCheckoutPlan
              ? t('upgrade.monthlyActiveLifetimeHint', 'Pro is active. Lifetime is available as a one-time upgrade.')
              : t('upgrade.monthlyActive', 'Pro is active.')}
          </p>
          {error && <p className="text-red-500 text-[12px] mt-2 text-center">{error}</p>}
        </div>
      ) : (
        <>
          {!user && (
            <p className="text-text-tertiary text-[12px] text-center mb-3">
              {t('upgrade.signInFirst')}
            </p>
          )}
          {error && <p className="text-red-500 text-[12px] mt-2 text-center">{error}</p>}
        </>
      )}
    </div>
  )
}

function QuotaBar({
  label,
  used,
  limit,
  unit,
  divisor,
}: {
  label: string
  used: number
  limit: number
  unit: string
  divisor: number
}) {
  const { t } = useTranslation()
  const pct = limit > 0 ? Math.min((used / limit) * 100, 100) : 0
  const usedDisplay = (used / divisor).toFixed(1)
  const limitDisplay = (limit / divisor).toFixed(1)

  return (
    <div className="space-y-1">
      <div className="flex justify-between text-[12px]">
        <span className="text-text-secondary">{label}</span>
        <span className="text-text-tertiary">
          {usedDisplay} / {limitDisplay} {unit}
        </span>
      </div>
      <div
        className="h-1.5 bg-bg-secondary rounded-full overflow-hidden"
        role="progressbar"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={t('account.quotaUsage', {
          label,
          used: usedDisplay,
          limit: limitDisplay,
          unit,
        })}
      >
        <div
          className={`h-full rounded-full transition-all ${pct > 90 ? 'bg-red-500' : 'bg-accent'}`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  )
}
