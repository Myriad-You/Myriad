/** App Store style list row. */

import type { CSSProperties } from 'react'
import type { UnifiedAppItem } from './types'
import { forwardRef, useEffect, useMemo, useState } from 'react'
import { Spinner } from '../../../components/Spinner'
import { useI18n } from '../../../contexts/I18nContext'
import { formatDownloadCount } from '../../utils/formatDownloadCount'
import {
  compareVersions,
  packageProgressLabel,
} from '../../utils/tappStoreHelpers'
import { TappIconBadge } from '../TappIconBadge'
import {
  getAppIconStyle,
  isOfficialStoreApp,
  OfficialVerifiedDot,
} from './storeAppMeta'
import { ProgressPercent, StoreGetButton } from './StoreChrome'

/** Shared clock so many rows don't each open their own interval. */
const SUBTITLE_ROTATE_MS = 3600
let subtitleTick = 0
const subtitleListeners = new Set<() => void>()
let subtitleIntervalId: ReturnType<typeof setInterval> | null = null

function subscribeSubtitleTick(listener: () => void): () => void {
  subtitleListeners.add(listener)
  subtitleIntervalId ??= setInterval(() => {
      subtitleTick += 1
      for (const fn of subtitleListeners) fn()
    }, SUBTITLE_ROTATE_MS)
  return () => {
    subtitleListeners.delete(listener)
    if (subtitleListeners.size === 0 && subtitleIntervalId != null) {
      clearInterval(subtitleIntervalId)
      subtitleIntervalId = null
    }
  }
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

/** One-line rotating subtitle: description → installs → updated-at. */
function useRotatingSubtitle(slides: string[], staggerIndex: number): string {
  const [, bump] = useState(0)
  const canRotate = slides.length > 1 && !prefersReducedMotion()

  useEffect(() => {
    if (!canRotate) return
    return subscribeSubtitleTick(() => bump((n) => n + 1))
  }, [canRotate])

  if (slides.length === 0) return ''
  if (!canRotate) return slides[0]!
  return slides[(subtitleTick + staggerIndex) % slides.length]!
}

export const UnifiedAppCard = forwardRef<
  HTMLDivElement,
  {
    app: UnifiedAppItem
    isInstalled: boolean
    installedVersion?: string
    date?: string
    onInstall: () => void
    onUpdate?: () => void
    /** Open store detail (row click). */
    onOpen: () => void
    /** Launch installed app (Get button when installed). */
    onLaunch: () => void
    installing: boolean
    installPercent?: number | null
    installPhase?: string | null
    installDetail?: string | null
    updating?: boolean
    /** Stagger index for CSS enter animation (`--as-enter-i`). */
    index?: number
  }
>(
  (
    {
      app,
      isInstalled,
      installedVersion,
      date,
      onInstall,
      onUpdate,
      onOpen,
      onLaunch,
      installing,
      installPercent,
      installPhase,
      installDetail,
      updating,
      index = 0,
    },
    ref,
  ) => {
    const { t, format, locale } = useI18n()
    const busy = installing || updating
    const busyProgress = installPercent != null && busy
    const hasUpdate =
      isInstalled &&
      !!installedVersion &&
      compareVersions(app.version, installedVersion) > 0

    const iconStyle = getAppIconStyle(app)
    const subtitleSlides = useMemo(() => {
      const slides: string[] = []
      const primary = (app.description || app.author.name || '').trim()
      if (primary) slides.push(primary)

      if (typeof app.downloads === 'number' && app.downloads > 0) {
        slides.push(
          format(t.tapp.downloadsCount, {
            n: formatDownloadCount(app.downloads, locale),
          }),
        )
      }

      if (date) {
        const dateValue = new Date(date)
        if (!Number.isNaN(dateValue.getTime())) {
          const startOfDay = (d: Date) =>
            new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
          const dayDiff = Math.floor(
            (startOfDay(new Date()) - startOfDay(dateValue)) / 86_400_000,
          )
          slides.push(
            dayDiff <= 0
              ? t.tapp.storeUpdatedToday
              : format(t.tapp.storeUpdatedDaysAgo, { n: dayDiff }),
          )
        }
      }

      return slides
    }, [
      app.description,
      app.author.name,
      app.downloads,
      date,
      format,
      locale,
      t.tapp.downloadsCount,
      t.tapp.storeUpdatedToday,
      t.tapp.storeUpdatedDaysAgo,
    ])

    const subtitle = useRotatingSubtitle(subtitleSlides, index)

    // CSS enter stagger (see TappStore.css). Cap keeps long lists cheap.
    const enterStyle = {
      ['--as-enter-i' as string]: Math.min(index, 14),
    } as CSSProperties

    return (
      <div
        ref={ref}
        className="as-store-row"
        style={enterStyle}
        role="button"
        tabIndex={0}
        title={t.tapp.viewDetails}
        onClick={onOpen}
        onKeyDown={(e: React.KeyboardEvent) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            onOpen()
          }
        }}
      >
        <TappIconBadge
          icon={app.icon}
          iconSvg={app.iconSvg}
          name={app.name}
          themeColor={app.themeColor}
          category={app.category}
          id={app.id}
          permissions={app.permissions}
          iconShell={app.iconShell}
          iconStyle={iconStyle}
          shellClassName="as-store-row__icon"
          glyphSizeClass="w-8 h-8"
          glyphTextClass="text-2xl"
        />

        <div className="as-store-row__body">
          <div className="as-store-row__name">
            {app.name}
            {isOfficialStoreApp(app) ? (
              <OfficialVerifiedDot label={t.tapp.official} />
            ) : null}
          </div>
          {subtitle ? (
            <div className="as-store-row__sub">
              <span key={subtitle} className="as-store-row__sub-text">
                {subtitle}
              </span>
            </div>
          ) : null}
        </div>

        <div className="as-store-row__side">
          {busy ? (
            <StoreGetButton
              kind="busy"
              disabled
              title={
                busyProgress
                  ? packageProgressLabel(
                      t.tapp,
                      updating ? 'update' : 'install',
                      installPhase,
                      installPercent!,
                      installDetail,
                    )
                  : updating
                    ? t.tapp.updating
                    : t.tapp.installing
              }
              label={
                busyProgress ? (
                  <ProgressPercent
                    value={installPercent!}
                    className="text-[0.75rem]"
                  />
                ) : (
                  <Spinner size="xs" color="current" />
                )
              }
            />
          ) : hasUpdate && onUpdate ? (
            <StoreGetButton
              kind="update"
              label={t.tapp.update}
              title={t.tapp.update}
              onClick={(e) => {
                e.stopPropagation()
                onUpdate()
              }}
            />
          ) : isInstalled ? (
            <StoreGetButton
              kind="open"
              label={t.tapp.start}
              title={t.tapp.start}
              onClick={(e) => {
                e.stopPropagation()
                onLaunch()
              }}
            />
          ) : (
            <StoreGetButton
              kind="get"
              label={t.tapp.install}
              title={t.tapp.install}
              onClick={(e) => {
                e.stopPropagation()
                onInstall()
              }}
            />
          )}
        </div>

        {busyProgress && (
          <div className="as-store-row__progress" aria-hidden>
            <i style={{ width: `${Math.max(2, installPercent!)}%` }} />
          </div>
        )}
      </div>
    )
  },
)

UnifiedAppCard.displayName = 'UnifiedAppCard'
