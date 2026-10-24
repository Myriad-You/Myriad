import type { ReactNode } from 'react'
import type { CompanionStatus } from '../../../services/phantasiaiApi'
import {
  LuHeadphones as Headphones,
  LuMessageSquare as MessageSquare,
  LuRefreshCw as RefreshCw,
} from '@lib/icons'
import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import * as phantasiaiApi from '../../../services/phantasiaiApi'
import { userFacingError } from '../../../utils/userFacingError'
import { NoteButton } from './NoteControls'

type Kind = 'annotations' | 'podcast'

/**
 * 已发布笔记的 AI 伴读：注释和播客各占一行，和系统设置里的列表一个样子。
 * 一行三段：图标（有了就是实心墨色）、名字和现状、右边一个动作。
 * 还没有的给「生成」，有了退成一个小小的刷新；生成中行底走一条细线，
 * 失败就写在那一行的说明位上，不连累另一行。生成后博客重建，文章页才挂上伴读岛。
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
      <h3 id="note-companion-title" className="note-section__title">{t.studio.companionTitle}</h3>
      {loadError ? (
        <p className="note-companion__load-error" role="alert">{loadError}</p>
      ) : (
        <ul className="note-companion__list">
          {cards.map((card) => {
            const working = busy === card.kind
            const failed = errors[card.kind]
            const state = !status ? 'is-loading' : working ? 'is-busy' : card.has ? 'is-ready' : 'is-empty'
            return (
              <li key={card.kind} className={`note-companion__row ${state}`} aria-busy={!status || working || undefined}>
                <span className="note-companion__icon" aria-hidden="true">{card.icon}</span>
                <span className="note-companion__text">
                  <span className="note-companion__label">
                    {card.label}
                    {status && !working ? (
                      <span key={card.value} className="note-companion__value">{card.value}</span>
                    ) : null}
                  </span>
                  <span className={`note-companion__desc${failed ? ' is-error' : ''}`} role={failed ? 'alert' : undefined}>
                    {failed || (working ? t.studio.companionGenerating : card.desc)}
                  </span>
                </span>
                {card.has ? (
                  <NoteButton
                    variant="quiet"
                    className="note-companion__action"
                    icon={<RefreshCw />}
                    loading={working}
                    disabled={!status || busy != null}
                    onClick={() => void generate(card.kind)}
                    aria-label={`${t.studio.companionRegenerate} · ${card.label}`}
                    title={t.studio.companionRegenerate}
                  />
                ) : (
                  <NoteButton
                    variant="outline"
                    className="note-companion__action"
                    loading={working}
                    disabled={!status || busy != null}
                    onClick={() => void generate(card.kind)}
                    aria-label={`${t.studio.companionGenerate} · ${card.label}`}
                  >
                    {t.studio.companionGenerate}
                  </NoteButton>
                )}
              </li>
            )
          })}
        </ul>
      )}
      <p className="note-field__hint">{t.studio.companionHint}</p>
    </section>
  )
}
