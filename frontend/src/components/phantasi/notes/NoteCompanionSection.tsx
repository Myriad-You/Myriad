import type { ReactNode } from 'react'
import type { CompanionStatus } from '../../../services/phantasiaiApi'
import {
  LuHeadphones as Headphones,
  LuMessageSquare as MessageSquare,
  LuRefreshCw as RefreshCw,
  LuSparkles as Sparkles,
} from '@lib/icons'
import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import * as phantasiaiApi from '../../../services/phantasiaiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { NoteButton } from './NoteControls'

type Kind = 'annotations' | 'podcast'

/**
 * 已发布笔记的 AI 伴读：注释和播客是两样互不相干的东西，各占一张卡。
 * 卡上依次是：它是什么、现在有没有、能做什么。没有的卡是虚线空位，等着被填上；
 * 生成中整张卡在流光，失败写在出错的那张卡里，不连累另一张。
 * 生成后博客重建，文章页才挂上伴读岛。
 */
export function NoteCompanionSection({ itemId }: { itemId: number }) {
  const { t, format } = useI18n()
  const [status, setStatus] = useState<CompanionStatus | null>(null)
  const [loadError, setLoadError] = useState('')
  const [busy, setBusy] = useState<Kind | null>(null)
  const [errors, setErrors] = useState<Partial<Record<Kind, string>>>({})

  useEffect(() => {
    const controller = new AbortController()
    setStatus(null)
    setLoadError('')
    setErrors({})
    phantasiaiApi.getCompanion(itemId, controller.signal).then(setStatus, (err: unknown) => {
      if (!controller.signal.aborted) setLoadError(userFacingError(err, t.studio.companionLoadFailed))
    })
    return () => controller.abort()
  }, [itemId, t.studio.companionLoadFailed])

  const generate = async (kind: Kind) => {
    if (busy || !status) return
    setBusy(kind)
    setErrors((current) => ({ ...current, [kind]: undefined }))
    try {
      const response =
        kind === 'annotations'
          ? status.annotations > 0
            ? await phantasiaiApi.regenerateAnnotations(itemId)
            : await phantasiaiApi.getAnnotations(itemId)
          : status.podcast
            ? await phantasiaiApi.regeneratePodcastScript(itemId)
            : await phantasiaiApi.getPodcastScript(itemId)
      if (!response.success) throw new Error(response.error || t.studio.companionFailed)
      setStatus(await phantasiaiApi.getCompanion(itemId))
    } catch (err) {
      setErrors((current) => ({ ...current, [kind]: userFacingError(err, t.studio.companionFailed) }))
    } finally {
      setBusy(null)
    }
  }

  const cards: Array<{ kind: Kind; icon: ReactNode; label: string; desc: string; has: boolean; value: string }> = [
    {
      kind: 'annotations',
      icon: <MessageSquare />,
      label: t.studio.companionAnnotationsLabel,
      desc: t.studio.companionAnnotationsDesc,
      has: (status?.annotations ?? 0) > 0,
      value: status && status.annotations > 0
        ? format(t.studio.companionAnnotationsCount, { count: status.annotations })
        : t.studio.companionNone,
    },
    {
      kind: 'podcast',
      icon: <Headphones />,
      label: t.studio.companionPodcastLabel,
      desc: t.studio.companionPodcastDesc,
      has: status?.podcast ?? false,
      value: status?.podcast ? t.studio.companionReady : t.studio.companionNone,
    },
  ]

  return (
    <section className="note-companion" aria-labelledby="note-companion-title">
      <header className="note-companion__head">
        <span className="note-companion__mark" aria-hidden="true"><Sparkles /></span>
        <h3 id="note-companion-title" className="note-companion__title">{t.studio.companionTitle}</h3>
      </header>
      <p className="note-field__hint">{t.studio.companionHint}</p>
      {loadError ? (
        <p className="note-companion__load-error" role="alert">{loadError}</p>
      ) : (
        <div className="note-companion__grid">
          {cards.map((card) => {
            const working = busy === card.kind
            const state = !status ? 'is-loading' : working ? 'is-busy' : card.has ? 'is-ready' : 'is-empty'
            return (
              <article key={card.kind} className={`note-companion__card ${state}`} aria-busy={!status || working || undefined}>
                <span className="note-companion__icon" aria-hidden="true">{card.icon}</span>
                <strong className="note-companion__label">{card.label}</strong>
                <span key={card.value} className="note-companion__value">
                  {!status ? '\u00A0' : working ? t.studio.companionGenerating : card.value}
                </span>
                <span className="note-companion__desc">{card.desc}</span>
                {errors[card.kind] ? (
                  <span className="note-companion__error" role="alert">{errors[card.kind]}</span>
                ) : null}
                <NoteButton
                  variant={card.has ? 'quiet' : 'solid'}
                  className="note-companion__action"
                  icon={card.has ? <RefreshCw /> : <Sparkles />}
                  loading={working}
                  disabled={!status || busy != null}
                  onClick={() => void generate(card.kind)}
                >
                  {card.has ? t.studio.companionRegenerate : t.studio.companionGenerate}
                </NoteButton>
              </article>
            )
          })}
        </div>
      )}
    </section>
  )
}
