/**
 * 资料库视图组件
 * 显示用户的多平台数据收藏
 */

import type { SecondaryNavItem } from '../contexts/NavigationContext'

import { useEffect, useMemo, useSyncExternalStore } from 'react'
import AnimatedView from '../components/AnimatedView'
import LibraryGrid from '../components/LibraryGrid'
import {
  isLibraryCanvasTourSurface,
  subscribeLibraryCanvasTourSurface,
} from '../components/tour/tourLogic'
import { useI18n } from '../contexts/I18nContext'
import { useSecondaryNav } from '../contexts/NavigationContext'
import { useLibraryScheduler } from '../hooks/animation'
import { usePageSeo } from '../hooks/usePageSeo'
import { buildModulePageSeo } from '../utils/modulePageSeo'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../utils/moduleVisibility'

// 资料库筛选图标
const FilterIcons = {
  all: (
    <svg
      className="w-5 h-5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
        d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z"
      />
    </svg>
  ),
  game: (
    <svg
      className="w-5 h-5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <rect x="2" y="6" width="20" height="12" rx="3" strokeWidth={2} />
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
        d="M6 12h4m-2-2v4"
      />
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={3}
        d="M15 11h.01M17 13h.01"
      />
    </svg>
  ),
  video: (
    <svg
      className="w-5 h-5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
        d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z"
      />
    </svg>
  ),
  music: (
    <svg
      className="w-5 h-5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
        d="M9 19V6l12-3v13M9 19c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zm12-3c0 1.105-1.343 2-3 2s-3-.895-3-2 1.343-2 3-2 3 .895 3 2zM9 10l12-3"
      />
    </svg>
  ),
  anime: (
    <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
      <text
        x="50%"
        y="50%"
        textAnchor="middle"
        dominantBaseline="central"
        fontSize="22"
        fontWeight="bold"
        className="font-sans"
      >
        あ
      </text>
    </svg>
  ),
  tv_series: (
    <svg
      className="w-5 h-5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
        d="M6 20.25h12m-7.5-3v3m3-3v3m-10.125-3h17.25c.621 0 1.125-.504 1.125-1.125V4.875c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v11.25c0 .621.504 1.125 1.125 1.125z"
      />
    </svg>
  ),
  book: (
    <svg
      className="w-5 h-5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={2}
        d="M4 19.5A2.5 2.5 0 016.5 17H20M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"
      />
    </svg>
  ),
}

type FilterType =
  'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'

export default function Library() {
  // 🆕 初始化资料库调度器（Resize + Intersection + Idle）
  useLibraryScheduler()
  const canvasTour = useSyncExternalStore(
    subscribeLibraryCanvasTourSurface,
    isLibraryCanvasTourSurface,
    isLibraryCanvasTourSurface,
  )

  const { t } = useI18n()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.library,
    { isAuthenticated: false, isAdmin: false },
  )

  usePageSeo(
    useMemo(
      () =>
        buildModulePageSeo({
          label: t.library.title || t.nav.library,
          description: t.widgets.multiPlatformAggregation,
          path: '/library',
          moduleOpenToAll,
        }),
      [t, moduleOpenToAll],
    ),
  )

  // 构建二级导航项
  const navItems: SecondaryNavItem[] = useMemo(
    () => [
      {
        id: 'all',
        icon: FilterIcons.all,
        label: t.nav.all,
        title: t.nav.all,
        ariaLabel: t.nav.showAll,
      },
      {
        id: 'game',
        icon: FilterIcons.game,
        label: t.nav.game,
        title: t.nav.game,
        ariaLabel: t.nav.showGame,
      },
      {
        id: 'video',
        icon: FilterIcons.video,
        label: t.nav.video,
        title: t.nav.video,
        ariaLabel: t.nav.showVideo,
      },
      {
        id: 'music',
        icon: FilterIcons.music,
        label: t.nav.music,
        title: t.nav.music,
        ariaLabel: t.nav.showMusic,
      },
      {
        id: 'anime',
        icon: FilterIcons.anime,
        label: t.nav.anime,
        title: t.nav.anime,
        ariaLabel: t.nav.showAnime,
      },
      {
        id: 'tv_series',
        icon: FilterIcons.tv_series,
        label: t.nav.tvSeries,
        title: t.nav.tvSeries,
        ariaLabel: t.nav.showTvSeries,
      },
      {
        id: 'book',
        icon: FilterIcons.book,
        label: t.nav.book,
        title: t.nav.book,
        ariaLabel: t.nav.showBook,
      },
    ],
    [t],
  )

  // 使用二级导航 Hook
  const { activeId, setExpanded } = useSecondaryNav({
    routePath: '/library',
    items: navItems,
    defaultActiveId: 'all',
    expandHint: t.nav.expandFilters,
  })

  // 监听展开事件
  useEffect(() => {
    const handleExpandSecondary = (e: CustomEvent<{ path: string }>) => {
      if (e.detail.path === '/library') {
        setExpanded(true)
      }
    }

    window.addEventListener(
      'nav-expand-secondary',
      handleExpandSecondary as EventListener,
    )
    return () => {
      window.removeEventListener(
        'nav-expand-secondary',
        handleExpandSecondary as EventListener,
      )
    }
  }, [setExpanded])

  // Filter chip engagement (not a new pageview — SPA stays on /library)
  useEffect(() => {
    if (!activeId || activeId === 'all') return
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.LIBRARY_FILTER, {
          target: activeId,
          throttleMs: 2000,
        })
      },
    )
  }, [activeId])

  return (
    <AnimatedView className="min-h-screen px-3 xs:px-4 sm:px-6 pt-20 pb-28 sm:pb-24 md:pb-12">
      <div
        className="max-w-7xl mx-auto w-full"
        data-tour={canvasTour ? undefined : 'library-grid'}
        data-tour-fit={
          canvasTour ? undefined : '.library-card-container, .library-empty'
        }
      >
        <LibraryGrid filter={activeId as FilterType} />
      </div>
    </AnimatedView>
  )
}
