/**
 * 首页视图组件
 * 显示可视化编辑的小组件网格
 */

import type { WidgetConfig, WidgetType } from '../components/WidgetGrid'
import { FaCog, FaEdit } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'

import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import { Avatar } from '../components/Avatar'
import { TitleFontSelector } from '../components/TitleFontSelector'
import WidgetGrid from '../components/WidgetGrid'
import {
  getBuiltinWidgets,
  preloadBuiltinWidgets,
} from '../components/widgets/builtinWidgets'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useHomeScheduler, usePageReady } from '../hooks/animation'
import { usePageSeo } from '../hooks/usePageSeo'
import { useBreakpoints } from '../hooks/useSharedEventListener'
import { useSiteOwnerProfile } from '../hooks/useSiteOwnerProfile'
import { useTappWidgets } from '../hooks/useTappWidgets'
import {
  useResolvedTitleColor,
  useTitleFont,
} from '../hooks/useTitleFont'
import { ensureMotionReady } from '../lib/lazyMotion'
import { buildHomePageSeo } from '../utils/modulePageSeo'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { hasSessionHint } from '../utils/sessionDetection'
import { showError } from '../utils/toastManager'
import { userFacingError } from '../utils/userFacingError'

export default function Home() {
  // 🆕 初始化首页调度器（Visibility + Resize + RAF + Idle）
  useHomeScheduler()

  const { isAuthenticated, hasChecked, checkAuth, isAdmin } = useAuth()
  const { t } = useI18n()
  const navigate = useNavigate()
  const isPageReady = usePageReady()
  // 与 WidgetGrid 列档同一套 viewportBands（phone≤767 / tablet / desktop≥1078）
  const { isMobile: isPhoneBand, isDesktop: isDesktopBand } = useBreakpoints()
  const isNotPhoneBand = !isPhoneBand

  // 站级 title/description；固定 canonical 为 /
  usePageSeo(useMemo(() => buildHomePageSeo(), []))
  const [widgets, setWidgets] = useState<WidgetConfig[]>([])
  const [isEditMode, setIsEditMode] = useState(false)
  // 站长资料：HTTP 层缓存 + avatar-changed 强制 no-store 刷新，换头像来源后立即同步
  const { profile: userInfo, avatarEpoch } = useSiteOwnerProfile({
    fallbackName: 'Myriad Dashboard',
    fallbackBio: t.home.defaultBio,
  })
  // 空字符串代表「尚未拿到服务端真实值」，不用写死的 'Dashboard' 占位
  // 文本，避免每个访客首次加载都要闪一下错误文字再跳到真实标题
  const [dashboardTitle, setDashboardTitle] = useState('')
  const [csrfToken, setCsrfToken] = useState<string>('')

  // 标题字体 Hook
  const { currentFont, titleFontSize } = useTitleFont()
  // 自适应色对齐 Tapp 音乐播放器歌词：对比度推导，随主题/壁纸色更新
  const titleColorCss = useResolvedTitleColor()

  // 默认小组件布局
  const DEFAULT_WIDGETS: WidgetConfig[] = useMemo(
    () => [
      {
        id: 'default-welcome',
        type: 'welcome',
        size: '4x2',
        position: { x: 0, y: 0 },
      },
      {
        id: 'default-weather',
        type: 'weather',
        size: '2x2',
        position: { x: 4, y: 0 },
      },
      {
        id: 'default-quote',
        type: 'quote',
        size: '2x2',
        position: { x: 6, y: 0 },
      },
      {
        id: 'default-agent-persona',
        type: 'agent-persona',
        size: '4x4',
        position: { x: 8, y: 0 },
      },
    ],
    [],
  )

  // Shared built-in catalog (same source as Control Panel)
  const AVAILABLE_WIDGETS: WidgetType[] = useMemo(
    () => getBuiltinWidgets(t.widgets, 'home'),
    [t.widgets],
  )

  // 获取 Tapp 注册的小组件
  const { tappWidgets, isLoading: isTappWidgetsLoading } = useTappWidgets()

  // 合并系统小组件和 Tapp 小组件
  const ALL_AVAILABLE_WIDGETS = useMemo(() => {
    return [...AVAILABLE_WIDGETS, ...tappWidgets]
  }, [AVAILABLE_WIDGETS, tappWidgets])

  // 智能检测：如果有登录迹象（会话提示标志），主动检查认证状态
  useEffect(() => {
    if (!hasChecked && hasSessionHint()) {
      // 检测到可能存在活跃会话，触发认证检查
      checkAuth()
    }
  }, [hasChecked, checkAuth])

  // 登录后获取 CSRF Token（强制从服务器拉，避免与 axios 轮换后的双缓存脱节）
  useEffect(() => {
    async function fetchCsrfToken() {
      if (isAuthenticated && hasChecked) {
        try {
          const { getCSRFToken } = await import('../utils/csrf')
          const { invalidateCsrfCache } = await import('../utils/userInfoCache')
          invalidateCsrfCache()
          const token = await getCSRFToken(true)
          if (token) {
            setCsrfToken(token)
          }
        } catch {
          // CSRF Token 获取失败时静默处理
        }
      }
    }
    fetchCsrfToken()
  }, [isAuthenticated, hasChecked])

  // 从后端加载小组件配置（使用去重机制）
  // 存储原始布局数据，用于 Tapp widgets 加载后重新验证
  const [rawLayoutData, setRawLayoutData] = useState<WidgetConfig[] | null>(
    null,
  )

  // motion 与配置请求并行；网格入场依赖真 motion，避免 shim 攒帧闪现
  useEffect(() => {
    void ensureMotionReady()
  }, [])

  useEffect(() => {
    // 挂载网格前预热：
    // - 布局内 lazy 小组件（shared Promise）
    // - 若含 report-*：整包报告卡（壳+全平台 face，禁止渲染期再拆）
    // - motion/react
    // 3s 超时兜底。
    async function applyWidgets(list: WidgetConfig[]) {
      await Promise.race([
        Promise.all([
          preloadBuiltinWidgets(list.map((w) => w.type)),
          ensureMotionReady(),
        ]),
        new Promise((resolve) => setTimeout(resolve, 3000)),
      ])
      setWidgets(list)
    }

    async function loadDashboardConfig() {
      try {
        const data = await getUIConfigDeduped()

        if (data.dashboard_layout) {
          try {
            const parsedLayout = JSON.parse(data.dashboard_layout)
            if (Array.isArray(parsedLayout)) {
              // 保存原始布局，待 Tapp widgets 加载后再过滤
              setRawLayoutData(parsedLayout)
              // 先用当前可用的组件过滤
              const registeredWidgetIds = new Set(
                ALL_AVAILABLE_WIDGETS.map((w) => w.id),
              )
              const loadedWidgets = parsedLayout.filter((w: WidgetConfig) =>
                registeredWidgetIds.has(w.type),
              )
              await applyWidgets(
                loadedWidgets.length > 0 ? loadedWidgets : DEFAULT_WIDGETS,
              )
            }
          } catch (e) {
            console.error('解析仪表盘布局失败:', e)
            await applyWidgets(DEFAULT_WIDGETS)
          }
        } else {
          await applyWidgets(DEFAULT_WIDGETS)
        }

        setDashboardTitle(data.dashboard_title || 'Dashboard')
      } catch (err) {
        console.error('加载配置失败:', err)
        await applyWidgets(DEFAULT_WIDGETS)
        setDashboardTitle('Dashboard')
      }
    }
    loadDashboardConfig()
  }, [])

  // 当 Tapp widgets 加载完成后，重新验证布局中的小组件
  useEffect(() => {
    if (isTappWidgetsLoading || !rawLayoutData || tappWidgets.length === 0)
      return

    // 使用完整的可用组件列表重新过滤
    const registeredWidgetIds = new Set(ALL_AVAILABLE_WIDGETS.map((w) => w.id))
    const validWidgets = rawLayoutData.filter((w: WidgetConfig) =>
      registeredWidgetIds.has(w.type),
    )

    if (validWidgets.length === 0) return

    // 成员未变时不要 setWidgets：新数组会牵动网格 index / 紧凑重排，
    // 曾导致进行中的入场动画被 skip 成 opacity 0（平台卡片「丢失」）。
    setWidgets((prev) => {
      if (
        prev.length === validWidgets.length &&
        prev.every(
          (w, i) =>
            w.id === validWidgets[i].id && w.type === validWidgets[i].type,
        )
      ) {
        return prev
      }
      return validWidgets
    })
  }, [isTappWidgetsLoading, tappWidgets, rawLayoutData, ALL_AVAILABLE_WIDGETS])

  // 保存小组件配置到后端
  const handleWidgetsChange = async (newWidgets: WidgetConfig[]) => {
    // 过滤未注册类型；Tapp catalog 仍 loading 时不要滤掉已存 Tapp 布局并 POST
    const registeredWidgetIds = new Set(ALL_AVAILABLE_WIDGETS.map((w) => w.id))
    const validWidgets = isTappWidgetsLoading
      ? newWidgets
      : newWidgets.filter((w) => registeredWidgetIds.has(w.type))

    setWidgets(validWidgets)

    // 只有管理员可以保存；catalog 未就绪时不写回，避免清空 Tapp 类型
    if (!isAdmin || isTappWidgetsLoading) return

    try {
      const { getCSRFToken } = await import('../utils/csrf')
      const token = (await getCSRFToken(true)) || csrfToken
      if (!token) {
        showError(t.errors.csrfUnavailable)
        return
      }
      if (token !== csrfToken) setCsrfToken(token)
      const res = await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': token,
        },
        credentials: 'include',
        body: JSON.stringify({
          layout: JSON.stringify(validWidgets), // 序列化为字符串存储
        }),
      })
      if (!res.ok) {
        throw new Error(`Failed to save dashboard layout: HTTP ${res.status}`)
      }
    } catch (err) {
      console.error('保存小组件配置失败:', err)
      showError(userFacingError(err, t.errors.dashboardLayoutSaveFailed))
    }
  }

  // 保存标题
  const handleTitleChange = async (newTitle: string) => {
    setDashboardTitle(newTitle)

    // 只有管理员可以保存
    if (!isAdmin) return

    try {
      const { getCSRFToken } = await import('../utils/csrf')
      const token = (await getCSRFToken(true)) || csrfToken
      if (!token) {
        showError(t.errors.csrfUnavailable)
        return
      }
      if (token !== csrfToken) setCsrfToken(token)
      const res = await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': token,
        },
        credentials: 'include',
        body: JSON.stringify({
          title: newTitle,
        }),
      })
      if (!res.ok) {
        throw new Error(`Failed to save dashboard title: HTTP ${res.status}`)
      }
    } catch (err) {
      console.error('保存标题失败:', err)
      showError(userFacingError(err, t.errors.dashboardTitleSaveFailed))
    }
  }

  // SocialNetworkWidget now persists custom platforms itself (with CSRF + ok check).
  // Keep a hardened fallback for any other publisher that only dispatches the event.
  useEffect(() => {
    const handleCustomPlatformsUpdate = async (event: Event) => {
      const customEvent = event as CustomEvent<{
        platforms: unknown[]
        persisted?: boolean
      }>
      if (customEvent.detail?.persisted) return
      if (!isAdmin) return

      try {
        const { getCSRFToken } = await import('../utils/csrf')
        const { clearDedupCache } = await import('../utils/requestDedup')
        const token = (await getCSRFToken(true)) || csrfToken
        if (!token) {
          showError(t.errors.csrfUnavailable)
          return
        }
        const response = await fetch(`${API_URL}/api/config/dashboard`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': token,
          },
          credentials: 'include',
          body: JSON.stringify({
            custom_platforms: JSON.stringify(customEvent.detail.platforms),
          }),
        })
        if (!response.ok) {
          throw new Error(
            `Failed to save custom platforms: HTTP ${response.status}`,
          )
        }
        clearDedupCache(`${API_URL}/api/config/ui`)
      } catch (err) {
        console.error('保存自定义平台失败:', err)
        showError(userFacingError(err, t.errors.customPlatformsSaveFailed))
      }
    }

    window.addEventListener(
      'custom-platforms-update',
      handleCustomPlatformsUpdate,
    )
    return () => {
      window.removeEventListener(
        'custom-platforms-update',
        handleCustomPlatformsUpdate,
      )
    }
  }, [isAdmin, csrfToken, t])

  return (
    <AnimatedView
      className={`home-shell min-h-screen ${
        isDesktopBand ? 'h-screen overflow-hidden' : ''
      }`}
      data-home-band={
        isDesktopBand ? 'desktop' : isPhoneBand ? 'phone' : 'tablet'
      }
    >
      <div className="home-shell__inner h-full flex flex-col pt-20 pb-6 px-3 xs:px-4 sm:px-6">
        <div className="home-shell__stage flex-1 max-w-7xl mx-auto w-full flex flex-col gap-4 p-2 relative min-h-0">
          {/* 小组件网格区域 - 占满整个可用空间 */}
          <WidgetGrid
            widgets={widgets}
            availableWidgets={ALL_AVAILABLE_WIDGETS}
            onWidgetsChange={handleWidgetsChange}
            isEditMode={isEditMode}
            onToggleEditMode={setIsEditMode}
          >
            {/* 顶部信息条 - 作为 children 传入 WidgetGrid */}
            <div className="relative h-15 shrink-0 z-10 mb-2 p-1">
              <h1 className="sr-only">{dashboardTitle}</h1>
              {/* 背景标题：phone 隐藏；tablet+desktop 显示（与 viewportBands 一致，非 Tailwind md 硬编码） */}
              {isEditMode ? (
                <input
                  type="text"
                  aria-label="Dashboard Title"
                  value={dashboardTitle}
                  onChange={(e) => handleTitleChange(e.target.value)}
                  className={`absolute left-1 whitespace-nowrap z-0 bg-transparent border-none outline-none p-0 m-0 w-full ${
                    isNotPhoneBand ? 'block' : 'hidden'
                  }`}
                  style={{
                    top: `calc(30px - ${7.5 * titleFontSize}rem)`,
                    fontSize: `${6 * titleFontSize}rem`,
                    color: titleColorCss,
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                    lineHeight: 1,
                    fontFamily: currentFont.family,
                    fontWeight: 700,
                  }}
                />
              ) : (
                <div
                  className={`absolute left-1 whitespace-nowrap pointer-events-none z-0 transition-opacity duration-300 ${
                    isNotPhoneBand ? 'block' : 'hidden'
                  }`}
                  style={{
                    top: `calc(30px - ${7.5 * titleFontSize}rem)`,
                    fontSize: `${6 * titleFontSize}rem`,
                    color: titleColorCss,
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                    fontFamily: currentFont.family,
                    fontWeight: 700,
                    // 服务端真实标题拿到前不显现，宁可留白也不闪错字
                    opacity: dashboardTitle ? 1 : 0,
                  }}
                >
                  {dashboardTitle}
                </div>
              )}

              <motion.div
                className="h-full flex items-center justify-between"
                initial={{ opacity: 0, x: -20 }}
                animate={
                  isPageReady ? { opacity: 1, x: 0 } : { opacity: 0, x: -20 }
                }
                transition={{
                  duration: 0.3,
                  ease: 'easeOut',
                  delay: isPageReady ? 0.1 : 0,
                }}
              >
                {/* 用户信息卡片 */}
                <motion.div
                  className="h-full glass rounded-xl px-4 py-1 flex items-center gap-3 shadow-sm relative z-10"
                  whileHover={{ scale: 1.02 }}
                  transition={{ duration: 0.2 }}
                >
                  {userInfo ? (
                    <>
                      <div className="w-8 h-8 rounded-full overflow-hidden border border-gray-200 dark:border-white/10">
                        <Avatar
                          // avatarEpoch：强制刷新后即使代理 URL 未变也 remount，避开 <img> 磁盘缓存
                          key={`${userInfo.avatar ?? ''}:${avatarEpoch}`}
                          src={userInfo.avatar}
                          name={userInfo.name}
                          className="w-full h-full object-cover"
                        />
                      </div>
                      <div className="flex flex-col justify-center">
                        <div className="text-sm font-bold text-gray-800 dark:text-gray-200 leading-tight">
                          {userInfo.name}
                        </div>
                        {userInfo.bio && (
                          <div className="text-[10px] text-gray-500 dark:text-gray-400 max-w-50 truncate leading-tight">
                            {userInfo.bio}
                          </div>
                        )}
                      </div>
                    </>
                  ) : (
                    <div className="flex items-center gap-2">
                      <div className="w-8 h-8 rounded-full bg-gray-200 dark:bg-white/5 animate-pulse" />
                      <div className="flex flex-col gap-1">
                        <div className="w-20 h-3 bg-gray-200 dark:bg-white/5 rounded animate-pulse" />
                        <div className="w-32 h-2 bg-gray-200 dark:bg-white/5 rounded animate-pulse" />
                      </div>
                    </div>
                  )}

                  {/* 编辑按钮 - 管理员 + desktop 档（≥1078，与 16 列网格同阈值） */}
                  {isAdmin && isDesktopBand && (
                    <>
                      <div className="h-6 w-px bg-gray-200 dark:bg-white/10 mx-1" />

                      {/* 字体选择器 - 仅在编辑模式下显示 */}
                      {isEditMode && (
                        <TitleFontSelector csrfToken={csrfToken} />
                      )}

                      <button
                        type="button"
                        onClick={() => setIsEditMode(!isEditMode)}
                        className={`
                          flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all
                          ${
                            isEditMode
                              ? 'text-white shadow-md hover:opacity-90'
                              : 'bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10'
                          }
                        `}
                        style={{
                          backgroundColor: isEditMode
                            ? 'var(--color-primary)'
                            : undefined,
                          color: isEditMode ? '#fff' : 'var(--color-primary)',
                        }}
                      >
                        <FaEdit size={12} />
                        {isEditMode ? t.common.done : t.common.edit}
                      </button>

                      {/* 配置入口 - 与编辑同条件：管理员 + desktop 档 */}
                      <button
                        type="button"
                        onClick={() => navigate('/config')}
                        className="flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
                        style={{ color: 'var(--color-primary)' }}
                        title={t.nav.config}
                        aria-label={t.nav.config}
                      >
                        <FaCog size={12} />
                        {t.nav.config}
                      </button>
                    </>
                  )}
                </motion.div>
              </motion.div>
            </div>
          </WidgetGrid>
        </div>
      </div>
    </AnimatedView>
  )
}
