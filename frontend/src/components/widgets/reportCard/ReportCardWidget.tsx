import type { CardContent } from './CardLogoPill'
import type { ReportCardClickAction, ReportCardWidgetProps } from './types'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { isPlainClick } from '../../../utils/plainClick'
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
import { useWidgetRotation } from '../shared/useWidgetRotation'
import { WidgetLongPressHint } from '../shared/WidgetLongPressHint'
import { WidgetPager } from '../shared/WidgetPager'
import { WidgetShell } from '../shared/WidgetShell'
import { WidgetSkeletonCover } from '../shared/WidgetSkeleton'
import { CardLogoPill } from './CardLogoPill'
import { PLATFORM_CONFIG } from './platformConfig'
import { PlatformFace } from './PlatformFace'
import { fetchPlatformUserIds, PLATFORM_SOCIAL } from './platformSocial'
import { buildReportCardPreviewData } from './previewData'
import {
  REPORT_ITEM_DWELL_MS,
  ReportDetailPagingContext,
} from './reportPaging'
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
    // 首页可交互的卡按真实页数翻：0 是总览，之后每页是详情里的一项（或一对）。
    const paged = !bare && !isPreview && !isOverviewControlled
    const [page, setPage] = useState(0)
    const [detailPaging, setDetailPaging] = useState({
      pages: 1,
      dwellMs: REPORT_ITEM_DWELL_MS,
    })
    // 换了平台就从总览开始；在渲染时就改，不留一帧「旧页码 + 新平台」。
    const [pagedPlatform, setPagedPlatform] = useState(platformId)
    if (pagedPlatform !== platformId) {
      setPagedPlatform(platformId)
      setPage(0)
    }
    const registerDetailPages = useCallback((pages: number, dwellMs: number) => {
      const next = Math.max(1, pages)
      setDetailPaging((prev) =>
        prev.pages === next && prev.dwellMs === dwellMs
          ? prev
          : { pages: next, dwellMs },
      )
    }, [])
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

    const pageCount = hasDetailContent
      ? paged
        ? 1 + detailPaging.pages
        : isOverviewControlled
          ? 1
          : 2
      : 1
    // 页数变少（数据刷新）时停在最后一页，不按取模跳到不相干的页；
    // 存着的页码也改掉，页数再变多时不会跳回去。
    const currentPage = Math.min(page, pageCount - 1)
    useEffect(() => {
      if (page > pageCount - 1) setPage(pageCount - 1)
    }, [page, pageCount])
    const showOverview = isOverviewControlled
      ? controlledShowOverview
      : paged
        ? currentPage === 0
        : internalShowOverview
    const detailPagingValue = useMemo(
      () => ({
        detailIndex: Math.max(0, currentPage - 1),
        register: registerDetailPages,
      }),
      [currentPage, registerDetailPages],
    )

    // 没有详情可翻、或这一档不自动轮换时回到总览；手动翻不受档位限制。
    useEffect(() => {
      if (isPreview || isOverviewControlled) return
      if (!hasDetailContent || !animLevel.widgetUiRotation) {
        setInternalShowOverview(true)
        setPage(0)
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

    // 首页：总览停 10 秒，详情每页按平台面报的时长；滑动、页码点都按真实页数翻。
    // 嵌在报告页（bare）时只在总览 ↔ 详情间自动翻，详情项由平台面自己轮换。
    const rotation = useWidgetRotation({
      count: pageCount,
      interactive: interactive && !isEditMode,
      delay: paged && currentPage > 0 ? detailPaging.dwellMs : 10000,
      autoplay:
        !isPreview &&
        !isOverviewControlled &&
        hasDetailContent &&
        animLevel.widgetUiRotation,
      onStep: (delta) => {
        if (!paged) {
          setInternalShowOverview((prev) => !prev)
          return
        }
        setPage((prev) => (Math.min(prev, pageCount - 1) + delta + pageCount) % pageCount)
      },
    })
    const shellRef = useCallback(
      (node: HTMLDivElement | null) => {
        localRef.current = node
        rotation.rootRef(node)
      },
      [rotation.rootRef],
    )
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
        onConfigChange?.(nextConfig)
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

    // 长按打开设置后松手的那次点击不算数；打开主页/报告页交给下面的链接。
    const handleCardClick = useCallback(() => {
      isLongPressRef.current = false
    }, [])

    // 卡片是真链接：右键复制、中键新开、长按菜单都归浏览器。
    const cardLink = useMemo(() => {
      if (!interactive || isEditMode) return null
      const social = PLATFORM_SOCIAL[platformId]
      if (clickAction === 'social' && socialUserId && social) {
        return {
          href: social.getUserUrl(socialUserId),
          external: true,
          label: `${social.publicName}: ${t.platformCard.clickToSocial}`,
        }
      }
      return {
        href: '/reports',
        external: false,
        label: t.platformCard.clickToReport,
      }
    }, [
      interactive,
      isEditMode,
      clickAction,
      socialUserId,
      platformId,
      t.platformCard,
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
        containerRef={shellRef}
        padding={0}
        contentClassName="contents"
        glass={!bare}
        className={`${interactive && !isEditMode ? 'cursor-pointer' : ''} ${rotation.rootClassName}`}
        rootProps={{
          ...rotation.rootProps,
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
            <ReportDetailPagingContext
              value={paged ? detailPagingValue : null}
            >
              <PlatformFace
                platformId={platformId}
                data={reportData}
                showOverview={showOverview}
                onContentChange={handleContentChange}
                allowLoop={animLevel.loop}
                paused={rotation.paused}
                isPreview={isPreview}
              />
            </ReportDetailPagingContext>
          </div>
        ) : null}

        {reportData ? (
          <CardLogoPill platformId={platformId} cardContent={cardContent} />
        ) : null}

        {cardLink ? (
          <a
            href={cardLink.href}
            target={cardLink.external ? '_blank' : undefined}
            rel={cardLink.external ? 'noopener noreferrer' : undefined}
            draggable={false}
            className="absolute inset-0 z-20"
            aria-label={cardLink.label}
            onClick={(event) => {
              event.stopPropagation()
              if (cardLink.external || !isPlainClick(event)) return
              event.preventDefault()
              navigate(cardLink.href)
            }}
          />
        ) : null}

        {reportData && rotation.active ? (
          <WidgetPager
            count={pageCount}
            index={paged ? currentPage : showOverview ? 0 : 1}
            placement="top"
            stopped={rotation.stopped}
            onToggleStopped={animLevel.widgetUiRotation ? rotation.toggleStopped : undefined}
            visible={rotation.showPager}
            onSelect={(index) => {
              if (paged) setPage(index)
              else setInternalShowOverview(index === 0)
              rotation.hold()
            }}
          />
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
