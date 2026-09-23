import type { DynamicContentType } from '../services/DynamicContentProvider'
import type { DynamicContent } from './ControlPanel/islandContentTypes'
import type { PanelTab } from './ControlPanel/panelTransition'
import React, {
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { useNavigate } from 'react-router-dom'
import { useAnimationPreference } from '../contexts/AnimationPreferenceContext'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { resetMeropeState } from '../features/merope/meropeAffectState'
import { setForegroundSurface } from '../features/merope/perception/surface'
import { useIdleEffect } from '../hooks/animation'
import { useAnimationLevel } from '../hooks/useAnimationLevel'

import { useMusicPlayer } from '../hooks/useMusicPlayer'
import { usePerformanceProfile } from '../hooks/usePerformanceProfile'
import { usePublicUiConfig } from '../hooks/usePublicUiConfig'
import { useThemePreference } from '../hooks/useThemePreference'

import { useVisibleState } from '../hooks/useVisibleState'
import { useWallpaper } from '../hooks/useWallpaper'
import { getDynamicContentProvider } from '../services/DynamicContentProvider'
import { useBackgroundResidents } from '../tapp/runtime/backgroundResidentStore'
import { emitAppEvent } from '../utils/appEvents'
import { lazyWithPreload } from '../utils/codeSplitting'
import {
  allowsIslandType,
  ISLAND_CONTENT_CHANGED_EVENT,
  islandContentFromPublicUi,
} from '../utils/islandContent'
import { formatMusicError } from '../utils/musicError'
import {
  getNavLayoutSnapshot,
  getServerNavLayoutSnapshot,
  subscribeNavLayout,
} from '../utils/navLayout'
import { useThemeMode } from '../utils/themeSubscriber'
import { showToast } from '../utils/toastManager'
import { ControlPanelWidgets } from './ControlPanel/ControlPanelWidgets'
import { ControlQuickActions } from './ControlPanel/ControlQuickActions'
import { islandPanelTabForClick } from './ControlPanel/islandClick'
import { MusicPlayerHost } from './ControlPanel/MusicPlayerHost'
import { trackPanelHeight } from './ControlPanel/panelHeight'
import {
  initialPanelState,
  isPanelMorphing,
  isPanelOpen,
  mountsNotifications,
  panelReducer,
  resolvePanelMotion,
  settleTimeoutMs,
  showsDynamicContent,
  showsOverlay,
  showsPanelContent,
  showsProgressUi,
} from './ControlPanel/panelTransition'
import { watchPanelTransition } from './ControlPanel/panelTransitionCompletion'
import { useBuiltinIslandContents } from './ControlPanel/useBuiltinIslandContents'
import { useControlPanelNotifications } from './ControlPanel/useControlPanelNotifications'
import { useIslandCarousel } from './ControlPanel/useIslandCarousel'
import { useIslandMusicContent } from './ControlPanel/useIslandMusicContent'
import { useIslandTextMotion } from './ControlPanel/useIslandTextMotion'
import { UserSection } from './ControlPanel/UserSection'
import { useTappIslandContents } from './ControlPanel/useTappIslandContents'
import { isHoverCapablePointer } from './ControlPanel/widgetCarousel'
import { homeBrowseTourPanelPose } from './tour/tourHomePose'
import {
  getTourSnapshot,
  subscribeTour,
} from './tour/tourStore'
import './GlobalControlPanel.css'

/**
 * Only the notifications tab renders this, so it stays out of the entry chunk;
 * an idle preload after first paint makes the first tab switch instant.
 */
const NotificationPanelList = lazyWithPreload(() => import('./NotificationPanelList'))

function readHomeBrowseTourPanelPose() {
  const snapshot = getTourSnapshot()
  return homeBrowseTourPanelPose(snapshot.tourId, snapshot.step?.id ?? null)
}

const GlobalControlPanel: React.FC = () => {
  const navigate = useNavigate()
  useIdleEffect(() => {
    NotificationPanelList.preload().catch(() => {})
  }, [], { priority: 'low' })
  const { user } = useAuth()
  useLayoutEffect(() => {
    resetMeropeState()
  }, [user?.id])
  const { locale, setLocale, t, format } = useI18n()
  const backgroundResidents = useBackgroundResidents()
  const navLayout = useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  // 触控带不挂天气/一言格，只留播放器与设置。
  const showControlPanelWidgets = navLayout === 'desktop'
  // 展开/收起唯一状态：phase（issue #320）。
  const [panel, dispatchPanel] = useReducer(panelReducer, initialPanelState)
  const isExpanded = isPanelOpen(panel)
  const showDynamicContent = showsDynamicContent(panel)
  const showPanelContent = showsPanelContent(panel)
  const showOverlay = showsOverlay(panel)
  const panelTab = panel.tab
  const isDark = useThemeMode()

  const [transientContents, updateTransientContents] = useVisibleState<DynamicContent[]>([])
  const [isHovering, setIsHovering] = useState(false)
  const uiConfig = usePublicUiConfig(ISLAND_CONTENT_CHANGED_EVENT)
  const islandContent = useMemo(() => islandContentFromPublicUi(uiConfig), [uiConfig])

  const anim = useAnimationLevel()
  const tappContents = useTappIslandContents()
  const builtinContents = useBuiltinIslandContents(user?.username, locale, t)
  const validContents = useMemo(() => {
    return [...transientContents, ...builtinContents, ...tappContents].filter((c) => {
      if (!c.icon || !c.text) return false
      if (typeof c.text === 'string' && c.text.trim().length === 0) return false
      if (!allowsIslandType(islandContent, String(c.type))) return false
      return true
    })
  }, [transientContents, builtinContents, tappContents, islandContent])
  const { currentContentIndex, setCurrentContentIndex, isTransitioning } = useIslandCarousel(
    validContents.length,
    isExpanded || isHovering,
    anim.durationScale,
  )

  const onIslandNotification = useCallback((content: DynamicContent | null) => {
    updateTransientContents(prev => {
      const rest = prev.filter(item => item.type !== 'notification')
      return content ? [content, ...rest] : rest
    })
    if (content) setCurrentContentIndex(0)
  }, [updateTransientContents])
  const { notifCenter, notificationPreferences } = useControlPanelNotifications({
    enabled: !!user,
    userId: user?.id,
    panelTab,
    onIslandNotification,
  })

  const {
    canRefresh: canRefreshWallpaper,
    refreshWallpaper,
    loadWallpaper,
  } = useWallpaper()

  const musicPlayer = useMusicPlayer()
  // 解构稳定引用，避免 expand/collapse 依赖整个 musicPlayer。
  const { setProgressUiVisible } = musicPlayer

  const triggerRef = useRef<HTMLDivElement>(null)
  const expandedContentRef = useRef<HTMLDivElement>(null)
  // 镜像给 popstate 等原生回调，避免闭包过期。
  const isExpandedRef = useRef(false)
  // 展开时压哨兵历史，系统返回先收起面板。
  const historyArmedRef = useRef(false)
  // 自己 history.back() 的 popstate 计数：关→开时迟到回退不得再收起。
  const pendingBackRef = useRef(0)
  // morphing 必须是组件级 ref，局部变量会随测量 effect 重建丢失。
  const morphingRef = useRef(false)
  const perf = usePerformanceProfile()

  // 同一套状态机按档位选 transition，不维护第二套交互。
  const motion = useMemo(
    () =>
      resolvePanelMotion({
        level: anim.level,
        reduceMotion: perf.reduceMotion,
      }),
    [anim.level, perf.reduceMotion],
  )
  // morph 开始时冻结档位跑完；中途降级会 cancel 尺寸过渡并误触发重测。
  const [activeMotion, setActiveMotion] = useState(motion)
  useEffect(() => {
    // 只在稳定态跟进档位，改档影响下一次交互。
    if (!isPanelMorphing(panel)) setActiveMotion(motion)
  }, [motion, panel.phase])

  const motionRef = useRef(activeMotion)
  useLayoutEffect(() => {
    motionRef.current = activeMotion
  }, [activeMotion])

  // 交接 delay/duration 按 morph 比例，两条时间线同步。
  const motionVars = useMemo(
    () =>
      ({
        '--gcp-morph': `${activeMotion.morphMs}ms`,
        '--gcp-tab': `${activeMotion.tabMs}ms`,
      }) as React.CSSProperties,
    [activeMotion],
  )

  const { preference: animPreference, togglePerformanceMode } =
    useAnimationPreference()
  const effectiveAnimationLevel =
    animPreference === 'auto' ? anim.level : animPreference
  const isStandardAnimation = effectiveAnimationLevel === 'standard'

  // layout effect 在同一次提交内、早于测量 effect 同步镜像。
  useLayoutEffect(() => {
    isExpandedRef.current = isPanelOpen(panel)
    historyArmedRef.current = panel.historyArmed
    morphingRef.current = isPanelMorphing(panel)
  }, [panel])

  useEffect(() => {
    loadWallpaper()
  }, []) // 只在挂载跑一次，避免循环依赖。

  const dynamicContentProvider = getDynamicContentProvider()

  useEffect(() => {
    dynamicContentProvider.setLocale(locale)
  }, [locale, dynamicContentProvider])

  useEffect(() => {
    musicPlayer.loadMusicConfig()
  }, [])

  useLayoutEffect(() => {
    if (!triggerRef.current) return
    const triggerEl = triggerRef.current

    if (!isExpanded) {
      triggerEl.style.height = '3rem'
      return
    }
    if (!expandedContentRef.current) return
    const contentEl = expandedContentRef.current

    return trackPanelHeight(triggerEl, contentEl, () => morphingRef.current)
  }, [isExpanded])

  const { themePreference, cycleThemePreference } = useThemePreference()
  const cycleTheme = useCallback(() => {
    const next = cycleThemePreference()
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.THEME_SWITCH, {
          target: next,
          throttleMs: 2000,
        })
      },
    )
  }, [cycleThemePreference])

  // 相位由外壳 transitionend 推进，定时器只兜底。
  useEffect(() => {
    if (!isPanelMorphing(panel)) return
    const el = triggerRef.current
    const generation = panel.generation
    const activeMotion = motionRef.current

    return watchPanelTransition(el, activeMotion.spatial, settleTimeoutMs(activeMotion), () => {
      emitAppEvent('gcp-animation-end')
      dispatchPanel({ type: 'settle', generation })
    })
  }, [panel.phase, panel.generation])

  // html.gcp-panel-open：移动端全屏 TApp 会抢 hit-test，用它关 TApp pointer-events。
  useEffect(() => {
    const root = document.documentElement
    if (isExpanded) {
      root.classList.add('gcp-panel-open')
    } else {
      root.classList.remove('gcp-panel-open')
    }
    return () => {
      root.classList.remove('gcp-panel-open')
    }
  }, [isExpanded])

  // 通知 Tab 控制区只 opacity 隐藏：进度/引擎需对齐可见性，展开首帧即置位。
  const progressUiVisible = showsProgressUi(panel)
  useEffect(() => {
    setProgressUiVisible(progressUiVisible)
  }, [progressUiVisible, setProgressUiVisible])

  const collapsePanel = useCallback(() => {
    dispatchPanel({ type: 'close' })
  }, [])

  const expandPanel = useCallback((tab?: PanelTab) => {
    // 压哨兵历史，系统返回先收起。保留 router state。同一 tick 立即置位，避免连开压出多余哨兵。
    let historyArmed = false
    if (!isExpandedRef.current) {
      isExpandedRef.current = true
      try {
        const st = window.history.state
        window.history.pushState(
          { ...(st ?? {}), idx: (st?.idx ?? 0) + 1, __gcpPanel: true },
          '',
        )
        historyArmed = true
      } catch {
        historyArmed = false
      }
      historyArmedRef.current = historyArmed
    }
    dispatchPanel({ type: 'open', tab, historyArmed })
  }, [])

  const handleClosePanel = useCallback(() => {
    if (!isExpandedRef.current) return
    if (historyArmedRef.current) {
      // 先解除武装再消费哨兵；pendingBack 必须计数，置 1 会把后续 popstate 当成用户返回。
      historyArmedRef.current = false
      pendingBackRef.current += 1
      window.history.back()
    }
    // 立即清镜像：同一 tick 关→开时第二次调用仍读到 true 会再关一次。
    isExpandedRef.current = false
    dispatchPanel({ type: 'close' })
    setForegroundSurface('none')
  }, [])

  const handleTogglePanel = useCallback(
    (tab?: PanelTab) => {
      if (isExpandedRef.current) {
        handleClosePanel()
      } else {
        expandPanel(tab)
        setForegroundSurface(
          tab === 'notifications' ? 'notification' : 'control_panel',
        )
        void import('../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.CONTROL_PANEL_OPEN, {
              throttleMs: 5000,
            })
          },
        )
      }
    },
    [handleClosePanel, expandPanel],
  )

  useEffect(() => {
    const handlePopState = () => {
      // 自己 history.back() 的 popstate 只记账，不改状态。
      if (pendingBackRef.current > 0) {
        pendingBackRef.current -= 1
        return
      }
      if (!historyArmedRef.current) return
      historyArmedRef.current = false
      dispatchPanel({ type: 'close' })
    }
    window.addEventListener('popstate', handlePopState)
    return () => window.removeEventListener('popstate', handlePopState)
  }, [])

  // 面板内导航：收起并用目标路由替换哨兵，不能 history.back()（异步回退会吞掉随后的 push）。
  const handleNavigateFromPanel = useCallback(
    (path: string) => {
      const wasArmed = historyArmedRef.current
      historyArmedRef.current = false
      collapsePanel()
      navigate(path, { replace: wasArmed })
    },
    [collapsePanel, navigate],
  )

  const handleOpenConfig = useCallback(() => {
    handleNavigateFromPanel('/config')
  }, [handleNavigateFromPanel])

  const handleOpenNotifSession = useCallback(
    (sessionId: string, opts?: { runId?: string; taskId?: string }) => {
      handleClosePanel()
      emitAppEvent('arael-open-session', {
            sessionId,
            runId: opts?.runId,
            taskId: opts?.taskId,
          })
    },
    [handleClosePanel],
  )

  const handleOpenAgentManage = useCallback(
    (tab?: 'heartbeat' | 'skills' | 'memory') => {
      handleClosePanel()
      emitAppEvent('arael-open-manage', tab ? { tab } : {})
    },
    [handleClosePanel],
  )

  useEffect(() => {
    const handleOpenPanel = (e: Event) => {
      const tab = (e as CustomEvent<{ tab?: PanelTab } | undefined>).detail?.tab
      if (isExpandedRef.current) {
        if (tab) dispatchPanel({ type: 'selectTab', tab })
        return
      }
      handleTogglePanel(tab)
    }

    window.addEventListener('open-control-panel', handleOpenPanel)
    return () => {
      window.removeEventListener('open-control-panel', handleOpenPanel)
    }
  }, [handleTogglePanel])

  const tourPanelPose = useSyncExternalStore(
    subscribeTour,
    readHomeBrowseTourPanelPose,
    readHomeBrowseTourPanelPose,
  )
  const tourDrovePanel = useRef(false)

  useLayoutEffect(() => {
    if (tourPanelPose === 'expanded') {
      tourDrovePanel.current = true
      if (!isExpandedRef.current) {
        expandPanel('control')
        setForegroundSurface('control_panel')
        return
      }
      if (panelTab !== 'control') {
        dispatchPanel({ type: 'selectTab', tab: 'control' })
      }
      return
    }
    if (tourPanelPose === 'collapsed') {
      if (isExpandedRef.current) handleClosePanel()
      tourDrovePanel.current = false
      return
    }
    if (tourDrovePanel.current) {
      if (isExpandedRef.current) handleClosePanel()
      tourDrovePanel.current = false
    }
  }, [expandPanel, handleClosePanel, panelTab, tourPanelPose])

  // 收起时内联错误不可见，用 toast 兜底；展开时不重复弹。
  const musicErrorKey = musicPlayer.musicErrorKey
  const musicErrorDetail = musicPlayer.musicErrorDetail
  useEffect(() => {
    if (!musicErrorKey || isExpandedRef.current) return
    const musicT = t.music as Record<string, string> | undefined
    showToast({
      message: formatMusicError(
        musicT?.[musicErrorKey] ?? musicErrorKey,
        musicErrorDetail,
      ),
      type: 'error',
      duration: 5000,
    })
    // t 不入依赖：只在错误出现时弹一次，语言切换不重弹。
  }, [musicErrorDetail, musicErrorKey])

  const onIslandMusic = useCallback((content: DynamicContent | null) => {
    updateTransientContents(previous => {
      const rest = previous.filter(item => item.type !== 'music')
      return content ? [content, ...rest] : rest
    })
  }, [updateTransientContents])
  useIslandMusicContent(musicPlayer, isExpanded, onIslandMusic)

  const safeContentIndex =
    validContents.length > 0
      ? Math.min(currentContentIndex, validContents.length - 1)
      : 0

  const currentContent =
    validContents.length > 0 ? validContents[safeContentIndex] : null

  const { textRef, needsScroll } = useIslandTextMotion(currentContent, !isExpanded)

  const shouldShowSubtext = useCallback((content: DynamicContent): boolean => {
    if (content.showSubtext !== undefined) {
      return content.showSubtext
    }

    if (content.type.toString().startsWith('tapp-')) {
      return !!content.subtext
    }

    const typesWithSubtext: DynamicContentType[] = ['weather', 'theme']
    return typesWithSubtext.includes(content.type)
  }, [])

  const hasValidContent = validContents.length > 0

  const notifCount = notifCenter.items.length
  const notificationIndicator =
    notifCount > 0 ? (
      <span className="dynamic-arrow-badge">
        {notifCount > 99 ? '99+' : notifCount}
      </span>
    ) : (
      <svg
        className="dynamic-arrow"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M19 9l-7 7-7-7"
        />
      </svg>
    )
  const residentLabel = format(t.controlPanel.residentRunning, {
    count: backgroundResidents.length,
  })
  const collapsedIndicator = (
    <span className="dynamic-status-indicators">
      {backgroundResidents.length > 0 && (
        <button
          type="button"
          className="tapp-resident-indicator"
          aria-label={residentLabel}
          title={residentLabel}
          onClick={(event) => {
            event.stopPropagation()
            handleTogglePanel(
              islandPanelTabForClick({ affordance: 'notification' }),
            )
          }}
        >
          {backgroundResidents.length}
        </button>
      )}
      <button
        type="button"
        className="dynamic-notification-affordance"
        aria-label={t.notificationCenter.title}
        onClick={(event) => {
          event.stopPropagation()
          handleTogglePanel(
            islandPanelTabForClick({ affordance: 'notification' }),
          )
        }}
      >
        {notificationIndicator}
      </button>
    </span>
  )

  return (
    <React.Fragment>
      <div className="global-control-bar">
        <div className="control-bar-content">
          <div
            ref={triggerRef}
            data-tour="control-island"
            className={[
              'control-bar-trigger',
              isExpanded ? 'expanded' : '',
              // morph 中冻结 hover/active 变换。
              isPanelMorphing(panel) ? 'gcp-animating' : '',
              panel.phase === 'closing' ? 'gcp-closing' : '',
              activeMotion.spatial ? '' : 'gcp-no-morph',
            ]
              .filter(Boolean)
              .join(' ')}
            style={motionVars}
            onPointerEnter={(e) => {
              if (isHoverCapablePointer(e.pointerType)) setIsHovering(true)
            }}
            onPointerLeave={() => setIsHovering(false)}
          >
            {hasValidContent && currentContent && (
              <div
                className={`dynamic-content-wrapper ${!showDynamicContent || isTransitioning ? 'hidden' : ''}`}
                onClick={() => {
                  handleTogglePanel(
                    islandPanelTabForClick({
                      affordance: 'control',
                      carouselType: currentContent.type,
                      viewport: navLayout,
                    }),
                  )
                }}
              >
                <span className="dynamic-icon">{currentContent.icon}</span>
                <div className="dynamic-text">
                  <span
                    ref={textRef}
                    className={`dynamic-text-main ${needsScroll ? 'scrolling' : ''}`}
                  >
                    {currentContent.text}
                  </span>
                  {currentContent.subtext &&
                    shouldShowSubtext(currentContent) && (
                      <span className="dynamic-text-sub">
                        {currentContent.subtext}
                      </span>
                    )}
                </div>
                {collapsedIndicator}
              </div>
            )}

            {/* 无内容也保留点击区；不按 isExpanded 卸载，否则展开首帧硬切。隐藏走绝对定位 + opacity:0。 */}
            {!hasValidContent && (
              <div
                className={`dynamic-content-wrapper empty-state ${!showDynamicContent ? 'hidden' : ''}`}
                onClick={() =>
                  handleTogglePanel(
                    islandPanelTabForClick({
                      affordance: 'control',
                      viewport: navLayout,
                    }),
                  )
                }
              >
                {collapsedIndicator}
              </div>
            )}

            <div
              ref={expandedContentRef}
              className={`expanded-panel-content ${showPanelContent ? 'visible' : ''}`}
              data-tour="control-panel"
            >
              <div className="control-panel-header">
                <UserSection
                  onClosePanel={handleClosePanel}
                  onNavigateFromPanel={handleNavigateFromPanel}
                />
                <button
                  type="button"
                  onClick={(e) => {
                    // 避免 touch 残留 focus / 父级 :active 干扰收起 morph。
                    e.stopPropagation()
                    ;(e.currentTarget as HTMLButtonElement).blur()
                    handleClosePanel()
                  }}
                  className="control-close-btn"
                  aria-label={t.common.close}
                >
                  <svg
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                    aria-hidden
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2.25}
                      d="M5 15l7-7 7 7"
                    />
                  </svg>
                </button>
              </div>

              <div
                className="notif-tab-bar"
                role="tablist"
                data-tab={panelTab}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={panelTab === 'control'}
                  className={`notif-tab ${panelTab === 'control' ? 'active' : ''}`}
                  onClick={() => {
                    dispatchPanel({ type: 'selectTab', tab: 'control' })
                    setForegroundSurface('control_panel')
                  }}
                >
                  {t.notificationCenter.tabControl}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={panelTab === 'notifications'}
                  className={`notif-tab ${panelTab === 'notifications' ? 'active' : ''}`}
                  onClick={() => {
                    dispatchPanel({ type: 'selectTab', tab: 'notifications' })
                    setForegroundSurface('notification')
                    void import('../utils/analyticsEvents').then(
                      ({ trackProductEvent, AnalyticsEvents }) => {
                        trackProductEvent(AnalyticsEvents.NOTIFICATION_OPEN, {
                          throttleMs: 5000,
                        })
                      },
                    )
                  }}
                >
                  {t.notificationCenter.title}
                  {notifCount > 0 && (
                    <span className="notif-tab-badge">
                      {notifCount > 99 ? '99+' : notifCount}
                    </span>
                  )}
                  {backgroundResidents.length > 0 && (
                    <span
                      className="tapp-resident-tab-badge"
                      aria-label={residentLabel}
                      title={residentLabel}
                    >
                      {backgroundResidents.length}
                    </span>
                  )}
                </button>
              </div>

              {/* 控制区始终挂载定高；切走用容器 opacity:0，勿 display:none（小组件会测成 0）或卸载。 */}
              <div className="notif-panel-body">
                <div
                  className={`notif-control-content ${
                    panelTab === 'notifications' ? 'inactive' : ''
                  }`}
                  inert={panelTab === 'notifications'}
                >
                  {showControlPanelWidgets && (
                    <ControlPanelWidgets
                      isAdmin={user?.is_admin}
                      panelVisible={progressUiVisible}
                    />
                  )}

                  <MusicPlayerHost
                    player={musicPlayer}
                    panelVisible={progressUiVisible}
                  />

                  <ControlQuickActions
                    locale={locale}
                    labels={t.controlPanel}
                    onLocaleChange={setLocale}
                    isDark={isDark}
                    themePreference={themePreference}
                    onCycleTheme={cycleTheme}
                    animPreference={animPreference}
                    animLevel={anim.level}
                    isStandardAnimation={isStandardAnimation}
                    onToggleAnimation={togglePerformanceMode}
                    canRefreshWallpaper={canRefreshWallpaper}
                    onRefreshWallpaper={refreshWallpaper}
                    isAdmin={!!user?.is_admin}
                    onOpenConfig={handleOpenConfig}
                  />
                </div>

                {/* 通知层 inset:0 跟高；首次进入后保持挂载，tab 往返交叉淡入；收起才卸载。 */}
                {mountsNotifications(panel) && (
                  <div
                    className={`notif-overlay ${panelTab === 'notifications' ? 'active' : ''}`}
                    inert={panelTab !== 'notifications'}
                  >
                    <Suspense fallback={null}>
                      <NotificationPanelList
                        center={notifCenter}
                        fill
                        onOpenSession={handleOpenNotifSession}
                        onNavigate={handleNavigateFromPanel}
                        onOpenAgentManage={handleOpenAgentManage}
                        browserNotificationsEnabled={
                          notificationPreferences.delivery.browser
                        }
                      />
                    </Suspense>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div
        className={[
          'control-panel-overlay',
          showOverlay ? 'visible' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        onClick={handleClosePanel}
      />
    </React.Fragment>
  )
}

export default GlobalControlPanel
