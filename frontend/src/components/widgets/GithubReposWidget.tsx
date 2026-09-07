/**
 * GitHub 仓库小组件：一张卡一个仓库。
 * 排版跟阅读器仓库卡同一套：头像 | 仓库名+owner | star，下面简介和语言。
 * 2x1 只留顶栏；2x2 / 4x2 出完整卡。
 */

import type { CSSProperties } from 'react'
import type { WidgetComponentProps } from '../widgetGridTypes'
import type { GithubRepoCardData } from '../../utils/githubRepo'
import { FaGithub, LuGitFork, LuStar } from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import {
  fetchGithubRepoCard,
  formatGithubCount,
  githubLanguageColor,
  githubOwnerAvatarUrl,
  githubRepoPageUrl,
  parseGithubRepoInput,
} from '../../utils/githubRepo'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'
import './GithubReposWidget.css'

function readConfiguredRepo(config: Record<string, unknown> | undefined): string {
  if (typeof config?.repo === 'string' && config.repo.trim()) return config.repo
  if (typeof config?.repos === 'string') {
    return config.repos.split(/[\n,]+/)[0] ?? ''
  }
  return ''
}

export const GithubReposWidget = memo(
  ({ config, isEditMode, isPreview }: WidgetComponentProps) => {
    const { t } = useI18n()
    const { containerRef, fontScale } = useWidgetSize(config.size)
    const compact = config.size === '2x1'
    const isTile = config.size === '2x2'
    const isWide = config.size === '4x2'
    const starsAtBottom = compact || isTile
    const raw = readConfiguredRepo(
      config.config as Record<string, unknown> | undefined,
    )
    const parsed = useMemo(() => parseGithubRepoInput(raw), [raw])
    const [data, setData] = useState<GithubRepoCardData | null>(null)
    const [loading, setLoading] = useState(!isPreview && Boolean(parsed))

    const preview = useMemo(
      () => ({
        owner: 'octocat',
        repo: t.githubReposWidget.sampleRepo,
        data: {
          stars: 12800,
          forks: 2100,
          description: t.githubReposWidget.sampleDesc,
          language: 'TypeScript',
        } satisfies GithubRepoCardData,
      }),
      [t.githubReposWidget],
    )

    const owner = isPreview ? preview.owner : parsed?.owner
    const repo = isPreview ? preview.repo : parsed?.repo
    const card = isPreview ? preview.data : data
    const ready = Boolean(owner && repo)

    useEffect(() => {
      if (isPreview || !parsed) {
        setData(null)
        setLoading(false)
        return undefined
      }
      let cancelled = false
      setLoading(true)
      void fetchGithubRepoCard(parsed)
        .then((next) => {
          if (!cancelled) {
            setData(next)
            setLoading(false)
          }
        })
        .catch(() => {
          if (!cancelled) {
            setData(null)
            setLoading(false)
          }
        })
      return () => {
        cancelled = true
      }
    }, [isPreview, parsed])

    const locked = isEditMode || isPreview
    const openRepo = useCallback(() => {
      if (locked || !parsed) return
      window.open(githubRepoPageUrl(parsed), '_blank', 'noopener,noreferrer')
    }, [locked, parsed])

    const titlePx = (compact ? 13 : isTile ? 14 : 15) * fontScale
    const ownerPx = (compact ? 10 : 11) * fontScale
    const bodyPx = (isTile ? 11 : 13) * fontScale
    const metaPx = (compact ? 11 : isTile ? 10 : 12) * fontScale
    const avatarPx = (compact ? 22 : 24) * fontScale

    const empty = !loading && !ready && !isPreview

    return (
      <WidgetShell
        containerRef={containerRef}
        padding={compact ? { x: 10, y: 8 } : { x: 12, y: 12 }}
        className={`select-none ${locked ? '' : 'cursor-pointer'} ${
          isEditMode ? 'cursor-grab' : ''
        }`}
        style={
          locked ? ({ pointerEvents: 'none' } as CSSProperties) : undefined
        }
        rootProps={
          ready && !locked
            ? {
                role: 'button',
                tabIndex: 0,
                onClick: openRepo,
                onKeyDown: (event: { key: string }) => {
                  if (event.key === 'Enter' || event.key === ' ') openRepo()
                },
              }
            : undefined
        }
        contentClassName="min-h-0 h-full"
      >
        {empty ? (
          <div className="github-repos-empty">
            <strong>{t.githubReposWidget.empty}</strong>
            <span>{t.githubReposWidget.emptyHint}</span>
          </div>
        ) : ready ? (
          <div
            className={`github-repos-card${compact ? ' is-compact' : ''}${isTile ? ' is-tile' : ''}${isWide ? ' is-wide' : ''}`}
          >
            <div className="github-repos-header">
              <span
                className="github-repos-avatar"
                style={{ width: avatarPx, height: avatarPx }}
              >
                <img
                  src={githubOwnerAvatarUrl(owner!)}
                  alt=""
                  loading="lazy"
                />
              </span>
              <span className="github-repos-text">
                <span
                  className="github-repos-title text-gray-800 dark:text-gray-100"
                  style={{ fontSize: titlePx }}
                >
                  {repo}
                </span>
                {compact ? (
                  card?.description ? (
                    <span
                      className="github-repos-owner text-gray-500 dark:text-gray-400"
                      style={{ fontSize: ownerPx }}
                    >
                      {card.description}
                    </span>
                  ) : null
                ) : (
                  <span
                    className="github-repos-owner text-gray-500 dark:text-gray-400"
                    style={{ fontSize: ownerPx }}
                  >
                    {owner}
                  </span>
                )}
              </span>
              {!starsAtBottom && card?.stars != null ? (
                <span
                  className="github-repos-stars"
                  style={{ fontSize: ownerPx }}
                >
                  <LuStar aria-hidden />
                  {formatGithubCount(card.stars)}
                </span>
              ) : null}
            </div>
            {!compact && card?.description ? (
              <p
                className="github-repos-desc text-gray-600 dark:text-gray-400"
                style={{ fontSize: bodyPx }}
              >
                {card.description}
              </p>
            ) : null}
            {compact && (card?.stars != null || card?.language) ? (
              <div
                className="github-repos-meta text-gray-500 dark:text-gray-400"
                style={{ fontSize: metaPx }}
              >
                {card?.stars != null ? (
                  <span>
                    <LuStar aria-hidden />
                    {formatGithubCount(card.stars)}
                  </span>
                ) : null}
                {card?.language ? (
                  <span>
                    <i
                      className="github-repos-lang"
                      style={{
                        background: githubLanguageColor(card.language),
                      }}
                    />
                    {card.language}
                  </span>
                ) : null}
              </div>
            ) : !compact &&
              (card?.language ||
                card?.forks != null ||
                (starsAtBottom && card?.stars != null)) ? (
              <div
                className="github-repos-meta text-gray-500 dark:text-gray-400"
                style={{ fontSize: metaPx }}
              >
                {starsAtBottom && card?.stars != null ? (
                  <span>
                    <LuStar aria-hidden />
                    {formatGithubCount(card.stars)}
                  </span>
                ) : null}
                {card?.forks != null ? (
                  <span>
                    <LuGitFork aria-hidden />
                    {formatGithubCount(card.forks)}
                  </span>
                ) : null}
                {card?.language ? (
                  <span>
                    <i
                      className="github-repos-lang"
                      style={{
                        background: githubLanguageColor(card.language),
                      }}
                    />
                    {card.language}
                  </span>
                ) : null}
              </div>
            ) : null}
            {isTile || compact ? null : (
              <FaGithub className="github-repos-mark" aria-hidden />
            )}
          </div>
        ) : null}
        <WidgetSkeletonCover
          active={loading}
          preset={compact ? 'media-row' : 'lines'}
          label={t.common.loading}
        />
      </WidgetShell>
    )
  },
)

GithubReposWidget.displayName = 'GithubReposWidget'
