import type { CardContent } from './CardLogoPill'
import type { ReportCardClickAction, ReportCardWidgetProps } from './types'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { useAnimationLevel } from '../../../hooks/useAnimationLevel'
import {
  coerceReportVisuals,
  hasRenderableCardVisuals,
  hasReportDetailContent,
  pickPlatformCardVisuals,
  resolveReportPlatformId,
} from '../../../utils/reportCardVisuals'
import { getLatestReportDeduped } from '../../../utils/requestDedup'
import { widgetDisplayLabel } from '../../widgetLibraryModel'
import { GlowBackground } from '../shared/GlowBackground'
import { WidgetLongPressHint } from '../shared/WidgetLongPressHint'
import { WidgetShell } from '../shared/WidgetShell'
import { WidgetSkeletonCover } from '../shared/WidgetSkeleton'
import { CardLogoPill } from './CardLogoPill'
import { PLATFORM_CONFIG } from './platformConfig'
import { PlatformFace } from './PlatformFace'
import { fetchPlatformUserIds, PLATFORM_SOCIAL } from './platformSocial'
import { buildReportCardPreviewData } from './previewData'
import {
  openReportCardSettingsModal,
  ReportCardSettingsModal,
} from './settingsModal'

export const ReportCardWidget = memo(
  ({
    config,
    isEditMode,
    isPreview,
    data: externalData,
    bare = false,
    showOverview: controlledShowOverview,
    onConfigChange,
  }: ReportCardWidgetProps) => {
    const animLevel = useAnimationLevel()
    const { t } = useI18n()
    const navigate = useNavigate()
    const localRef = useRef<HTMLDivElement | null>(null)
    const platformId = resolveReportPlatformId(config)
    const [reportData, setReportData] = useState<any>(null)
    const [loading, setLoading] = useState(true)
    const [failed, setFailed] = useState(false)
    const isOverviewControlled = controlledShowOverview !== undefined
    const [internalShowOverview, setInternalShowOverview] = useState(true)
    const showOverview = isOverviewControlled
      ? controlledShowOverview
      : internalShowOverview
    const [cardContent, setCardContent] = useState<CardContent>(null)

    useEffect(() => {
      if (isPreview) {
        setReportData(buildReportCardPreviewData(platformId, t))
        setLoading(false)
        return
      }

      if (externalData !== undefined) {
        const visuals = coerceReportVisuals(externalData)
        setReportData(hasRenderableCardVisuals(visuals) ? visuals : null)
        setLoading(false)
        return
      }

      // 取消的 fetch 不能把 loading 留在已卸载实例上。
      let cancelled = false
      const fetchReport = async (forceRefresh = false) => {
        try {
          let data = await getLatestReportDeduped({ forceRefresh })
          if (cancelled) return
          // 映射空/不对就失败，首页不要挂空白壳。
          let visuals = pickPlatformCardVisuals(data, platformId)
          if (!visuals && !forceRefresh) {
            data = await getLatestReportDeduped({ forceRefresh: true })
            if (cancelled) return
            visuals = pickPlatformCardVisuals(data, platformId)
          }
          setReportData(visuals)
          setFailed(false)
        } catch (err) {
          if (cancelled) return
          console.error(`${t.reportCardWidget.fetchReportFailed}:`, err)
          setReportData(null)
          setFailed(true)
        } finally {
          if (!cancelled) setLoading(false)
        }
      }
      fetchReport()

      let timeoutId: number | null = null
      const schedule = () => {
        if (cancelled || document.hidden) return
        fetchReport()
        timeoutId = window.setTimeout(schedule, 5 * 60 * 1000)
      }
      timeoutId = window.setTimeout(schedule, 5 * 60 * 1000)

      const onVisibility = () => {
        if (document.hidden && timeoutId) {
          clearTimeout(timeoutId)
          timeoutId = null
        } else if (!document.hidden && !cancelled && !timeoutId) {
          schedule()
        }
      }
      document.addEventListener('visibilitychange', onVisibility)

      return () => {
        cancelled = true
        if (timeoutId) clearTimeout(timeoutId)
        document.removeEventListener('visibilitychange', onVisibility)
      }
    }, [platformId, isPreview, externalData])

    const hasDetailContent = useMemo(
      () => hasReportDetailContent(reportData),
      [reportData],
    )

    useEffect(() => {
      if (isPreview || isOverviewControlled) return
      if (!hasDetailContent) {
        setInternalShowOverview(true)
        return
      }
      if (!animLevel.widgetUiRotation) {
        setInternalShowOverview(true)
        return
      }

      let cancelled = false
      let timeoutId: number | null = null
      const tick = () => {
        if (cancelled || document.hidden) return
        setInternalShowOverview((prev) => !prev)
        timeoutId = window.setTimeout(tick, 10000)
      }
      timeoutId = window.setTimeout(tick, 10000)

      const onVisibility = () => {
        if (document.hidden && timeoutId) {
          clearTimeout(timeoutId)
          timeoutId = null
        } else if (!document.hidden && !cancelled && !timeoutId) {
          tick()
        }
      }
      document.addEventListener('visibilitychange', onVisibility)

      return () => {
        cancelled = true
        if (timeoutId) clearTimeout(timeoutId)
        document.removeEventListener('visibilitychange', onVisibility)
      }
    }, [
      isPreview,
      isOverviewControlled,
      hasDetailContent,
      animLevel.widgetUiRotation,
    ])

    const handleContentChange = useCallback((content: any) => {
      setCardContent(content)
    }, [])

    const interactive = !bare && !isPreview
    const clickAction: ReportCardClickAction =
      config.config?.clickAction === 'social' ? 'social' : 'report'

    const [socialUserId, setSocialUserId] = useState<string | undefined>(
      undefined,
    )
    useEffect(() => {
      if (!interactive || clickAction !== 'social') return
      let alive = true
      fetchPlatformUserIds().then((m) => {
        if (alive) setSocialUserId(m[platformId])
      })
      return () => {
        alive = false
      }
    }, [interactive, clickAction, platformId])

    const isLongPressRef = useRef(false)
    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

    const applyClickAction = useCallback(
      (action: ReportCardClickAction) => {
        const nextConfig = { ...config.config, platformId, clickAction: action }
        if (typeof onConfigChange === 'function') {
          onConfigChange(nextConfig)
        } else {
          window.dispatchEvent(
            new CustomEvent('widget-config-update', {
              detail: {
                widgetId: config.id,
                config: nextConfig,
              },
            }),
          )
        }
        isLongPressRef.current = false
      },
      [config.id, config.config, onConfigChange, platformId],
    )

    const openSettings = useCallback(() => {
      if (!localRef.current) return
      openReportCardSettingsModal(
        clickAction,
        localRef.current.getBoundingClientRect(),
        applyClickAction,
        () => {
          isLongPressRef.current = false
        },
        widgetDisplayLabel(
          { id: config.type, name: config.type },
          t.widgets as unknown as Record<string, unknown>,
        ),
      )
    }, [applyClickAction, clickAction, config.type, t.widgets])

    const handlePressStart = useCallback(() => {
      if (!interactive || !isEditMode) return
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
      }
      isLongPressRef.current = false
      longPressTimerRef.current = setTimeout(() => {
        longPressTimerRef.current = null
        isLongPressRef.current = true
        openSettings()
      }, 500)
    }, [interactive, isEditMode, openSettings])

    const handlePressEnd = useCallback(() => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
        longPressTimerRef.current = null
      }
    }, [])

    useEffect(() => {
      return () => {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current)
        }
        isLongPressRef.current = false
      }
    }, [])

    const handleCardClick = useCallback(() => {
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }
      if (!interactive || isEditMode) return
      if (clickAction === 'social' && socialUserId) {
        window.open(
          PLATFORM_SOCIAL[platformId]?.getUserUrl(socialUserId) || '#',
          '_blank',
          'noopener,noreferrer',
        )
        return
      }
      navigate('/reports')
    }, [
      interactive,
      isEditMode,
      clickAction,
      socialUserId,
      platformId,
      navigate,
    ])

    const handleMouseLeave = useCallback(() => {
      handlePressEnd()
    }, [handlePressEnd])

    const platformConfig =
      PLATFORM_CONFIG[platformId] || PLATFORM_CONFIG.bilibili

    if (!loading && !reportData) {
      return (
        <div className="h-full w-full flex items-center justify-center text-gray-400 text-sm">
          <span>
            {failed
              ? t.reportCardWidget.fetchReportFailed
              : t.reportCard.noReportData}
          </span>
        </div>
      )
    }

    return (
      <WidgetShell
        containerRef={localRef}
        padding={0}
        contentClassName="contents"
        glass={!bare}
        className={interactive && !isEditMode ? 'cursor-pointer' : ''}
        rootProps={{
          onClick: interactive ? handleCardClick : undefined,
          onMouseDown: interactive ? handlePressStart : undefined,
          onMouseUp: interactive ? handlePressEnd : undefined,
          onMouseLeave: interactive ? handleMouseLeave : undefined,
          onTouchStart: interactive ? handlePressStart : undefined,
          onTouchEnd: interactive ? handlePressEnd : undefined,
          onTouchCancel: interactive ? handlePressEnd : undefined,
        }}
        background={
          !bare && (
            <GlowBackground
              color={platformConfig.color}
              animLevel={animLevel.level}
              shouldAnimate={
                !loading && animLevel.loop && animLevel.widgetGlow
              }
              variant="single"
              size="lg"
            />
          )
        }
      >
        {reportData ? (
          <div className="absolute inset-0 z-10 flex min-h-0 flex-col">
            <PlatformFace
              platformId={platformId}
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
              allowLoop={animLevel.loop}
              isPreview={isPreview}
            />
          </div>
        ) : null}

        {reportData ? (
          <CardLogoPill platformId={platformId} cardContent={cardContent} />
        ) : null}

        <WidgetSkeletonCover
          active={loading}
          preset="report"
          accent={{
            color: platformConfig.color,
            soft: platformConfig.bgColor,
          }}
          label={t.common.loading}
        />

        <WidgetLongPressHint
          visible={interactive && isEditMode}
          title={t.platformCard.longPressHint}
          onClick={openSettings}
        />
      </WidgetShell>
    )
  },
)

ReportCardWidget.displayName = 'ReportCardWidget'

export { ReportCardSettingsModal }
