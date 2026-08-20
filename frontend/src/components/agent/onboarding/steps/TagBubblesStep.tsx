import type { LifeOnboardingTag, OnboardingHeaderChrome } from '../onboardingTypes'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { agentService } from '../../../../services/agent'
import { ApiError } from '../../../../services/api'
import { keepSelectedPersonaTags, uniqPersonaTags } from '../../personaTags'
import {
  generationCacheKey,
  getGenerationCache,
  setGenerationCache,
} from '../generationCache'
import { generationFailureMessage } from '../generationError'
import BubbleCanvas from '../ui/BubbleCanvas'
import { ActionBar, PrimaryButton } from '../ui/Chrome'
import { ErrorNote, Working } from '../ui/Feedback'

interface SignalsCache {
  tags: LifeOnboardingTag[]
  reportCount: number
  aiDistilled: boolean
}

interface Props {
  selected: string[]
  onChange: (tags: string[]) => void
  onNext: () => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
}

function signalsCacheKey(locale: string) {
  return generationCacheKey('signals', [locale])
}

function toTags(labels: string[]): LifeOnboardingTag[] {
  const stamp = Date.now().toString(36)
  return uniqPersonaTags(labels).map((label, index) => ({
    id: `${stamp}-${index}`,
    label,
    weight: Math.max(0.35, 1 - index * 0.08),
  }))
}

export default function TagBubblesStep({
  selected,
  onChange,
  onNext,
  onHeaderChange,
}: Props) {
  const { t, locale } = useI18n()
  const o = t.life.onboarding
  const cacheKey = signalsCacheKey(locale)
  const cached = getGenerationCache<SignalsCache>(cacheKey)
  const [tags, setTags] = useState<LifeOnboardingTag[]>(() => cached?.tags || [])
  const [reportCount, setReportCount] = useState(() => cached?.reportCount || 0)
  const [aiDistilled, setAiDistilled] = useState(
    () => Boolean(cached?.aiDistilled),
  )
  const [loading, setLoading] = useState(() => !cached)
  const [regenerating, setRegenerating] = useState(false)
  const [error, setError] = useState('')
  const [reloadToken, setReloadToken] = useState(0)
  const [canPan, setCanPan] = useState(false)
  const tagsRef = useRef(tags)
  tagsRef.current = tags

  useEffect(() => {
    let cancelled = false
    const force = reloadToken > 0
    const existing = getGenerationCache<SignalsCache>(cacheKey)
    if (!force && existing?.tags?.length) {
      setTags(existing.tags)
      setReportCount(existing.reportCount)
      setAiDistilled(existing.aiDistilled)
      setLoading(false)
      setRegenerating(false)
      const kept = keepSelectedPersonaTags(
        selected,
        existing.tags.map((tag) => tag.label),
      )
      if (kept.length !== selected.length) onChange(kept)
      return
    }

    if (force) setRegenerating(true)
    else setLoading(true)

    void agentService
      .getPersonaSignals({ language: locale, regenerate: force })
      .then((signals) => {
        if (cancelled) return
        const nextTags = toTags(signals.tags)
        const next: SignalsCache = {
          tags: nextTags,
          reportCount: signals.reportCount,
          aiDistilled: signals.aiDistilled === true,
        }
        setTags(next.tags)
        setReportCount(next.reportCount)
        setAiDistilled(next.aiDistilled)
        setGenerationCache(cacheKey, next)
        setError('')
        if (nextTags.length > 0) {
          const kept = keepSelectedPersonaTags(
            selected,
            nextTags.map((tag) => tag.label),
          )
          if (kept.length !== selected.length) onChange(kept)
        }
      })
      .catch((reason) => {
        if (cancelled) return
        if (!force && tagsRef.current.length > 0) return
        if (reason instanceof ApiError && reason.code === 'agent_life_disabled') {
          setError(o.saveFirst)
          return
        }
        setError(
          generationFailureMessage(
            reason,
            o.loadSignalsFailed,
            o.generationTimeout,
          ),
        )
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
          setRegenerating(false)
        }
      })
    return () => {
      cancelled = true
    }
    // selected/onChange 只在换一批后修剪，不跟进当前勾选。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cacheKey, locale, o.generationTimeout, o.loadSignalsFailed, o.saveFirst, reloadToken])

  const reshuffle = useCallback(() => {
    setReloadToken((token) => token + 1)
  }, [])

  const toggleById = useCallback(
    (id: string) => {
      const hit = tags.find((tag) => tag.id === id)
      if (!hit) return
      if (selected.includes(hit.label)) {
        onChange(selected.filter((item) => item !== hit.label))
      } else {
        onChange([...selected, hit.label])
      }
    },
    [onChange, selected, tags],
  )

  const canvasKey = useMemo(
    () => tags.map((tag) => tag.id).join('|') || 'empty',
    [tags],
  )
  const canvasItems = useMemo(
    () =>
      tags.map((tag) => ({
        id: tag.id,
        label: tag.label,
        weight: Math.min(Math.max(tag.weight, 0), 1),
        selected: selected.includes(tag.label),
        disabled: loading || regenerating,
      })),
    [loading, regenerating, selected, tags],
  )

  const tagLabels = new Set(tags.map((tag) => tag.label))
  const selectedCount = selected.reduce(
    (count, label) => count + (tagLabels.has(label) ? 1 : 0),
    0,
  )
  const canContinue = selectedCount > 0
  const blocked = loading || regenerating
  const sourceNote = aiDistilled
    ? o.aiDistilledMeta.replace('{count}', String(reportCount))
    : reportCount > 0
      ? o.reportsButFallback.replace('{count}', String(reportCount))
      : o.noReports

  useLayoutEffect(() => {
    onHeaderChange({
      description: loading ? o.step1Lead : sourceNote,
      action: {
        label: regenerating ? o.regeneratingSeeds : o.regenerateSeeds,
        busy: loading || regenerating,
        disabled: blocked,
        onClick: reshuffle,
      },
    })
  }, [
    blocked,
    loading,
    o.regenerateSeeds,
    o.regeneratingSeeds,
    o.step1Lead,
    onHeaderChange,
    regenerating,
    reshuffle,
    sourceNote,
  ])

  return (
    <section className="life-ob-tags" aria-label={o.step1Title}>
      <div className="life-ob-tags__board">
        {loading || regenerating ? (
          <Working>
            {regenerating ? o.regeneratingSeeds : o.loadingAiSignals}
          </Working>
        ) : canvasItems.length > 0 ? (
          <BubbleCanvas
            key={canvasKey}
            label={o.step1Title}
            items={canvasItems}
            onPannableChange={setCanPan}
            onToggle={toggleById}
          />
        ) : null}
        <div
          className="life-ob-tags__fade life-ob-tags__fade--bottom"
          aria-hidden
        />
      </div>
      {error && (
        <div className="life-ob-tags__error">
          <ErrorNote>{error}</ErrorNote>
        </div>
      )}
      {!loading && !regenerating && !error && tags.length === 0 && (
        <div className="life-ob-tags__error">
          <ErrorNote>{o.signalsEmpty}</ErrorNote>
        </div>
      )}
      {!loading && (
        <p className="life-ob-tags__status" aria-live="polite">
          <span>
            {selectedCount > 0
              ? o.selectedCount.replace('{count}', String(selectedCount))
              : o.selectNothingYet}
          </span>
          {canPan && (
            <>
              <span className="life-ob-tags__status-sep" aria-hidden>
                ·
              </span>
              <span className="life-ob-tags__status-pan">{o.dragCanvas}</span>
            </>
          )}
        </p>
      )}
      <ActionBar>
        <PrimaryButton
          label={o.next}
          disabled={!canContinue || blocked}
          onClick={onNext}
        />
      </ActionBar>
    </section>
  )
}
