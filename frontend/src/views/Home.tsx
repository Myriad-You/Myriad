/**
 * 首页视图组件
 * 显示可视化编辑的小组件网格
 */

import type { WidgetConfig, WidgetType } from '../components/WidgetGrid'
import type { UserInfo } from '../utils/userInfoCache'
import { FaEdit } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'

import { useEffect, useMemo, useState } from 'react'
import AnimatedView from '../components/AnimatedView'
import { TitleFontSelector } from '../components/TitleFontSelector'
import WidgetGrid from '../components/WidgetGrid'
import { getBuiltinWidgets } from '../components/widgets/builtinWidgets'
import { API_URL } from '../config'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useHomeScheduler, usePageReady } from '../hooks/animation'
import { useTappWidgets } from '../hooks/useTappWidgets'
import {
  useResolvedTitleColor,
  useTitleFont,
} from '../hooks/useTitleFont'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { hasSessionHint } from '../utils/sessionDetection'
import {
  getCsrfTokenWithCache,
  getUserInfoWithCache,
} from '../utils/userInfoCache'

export default function Home() {
  // 🆕 初始化首页调度器（Visibility + Resize + RAF + Idle）
  useHomeScheduler()

  const { isAuthenticated, hasChecked, checkAuth, isAdmin } = useAuth()
  const { t } = useI18n()
  const isPageReady = usePageReady()
  const [widgets, setWidgets] = useState<WidgetConfig[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [isEditMode, setIsEditMode] = useState(false)
  const [userInfo, setUserInfo] = useState<UserInfo | null>(null)
  const [dashboardTitle, setDashboardTitle] = useState('Dashboard')
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
    ],
    [],
  )

  // Shared built-in catalog (same source as Control Panel)
  const AVAILABLE_WIDGETS: WidgetType[] = useMemo(
    () => getBuiltinWidgets(t.widgets),
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

  // 获取用户信息（使用缓存）
  useEffect(() => {
    async function fetchUserInfo() {
      try {
        // 总是获取公开用户信息（站长资料），不需要等待认证检查
        const info = await getUserInfoWithCache(true) // 跳过认证检查
        setUserInfo(info)
      } catch {
        // 设置默认访客信息
        setUserInfo({
          name: 'Myriad Dashboard',
          avatar: 'https://ui-avatars.com/api/?name=Myriad&background=random',
          bio: t.home.defaultBio,
          is_admin: false,
        })
      }
    }
    fetchUserInfo()
  }, [t])

  // 登录后获取 CSRF Token
  useEffect(() => {
    async function fetchCsrfToken() {
      if (isAuthenticated && hasChecked) {
        try {
          const token = await getCsrfTokenWithCache()
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

  useEffect(() => {
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
              setWidgets(
                loadedWidgets.length > 0 ? loadedWidgets : DEFAULT_WIDGETS,
              )
            }
          } catch (e) {
            console.error('解析仪表盘布局失败:', e)
            setWidgets(DEFAULT_WIDGETS)
          }
        } else {
          setWidgets(DEFAULT_WIDGETS)
        }

        if (data.dashboard_title) {
          setDashboardTitle(data.dashboard_title)
        }
      } catch (err) {
        console.error('加载配置失败:', err)
        setWidgets(DEFAULT_WIDGETS)
      } finally {
        setIsLoading(false)
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

    if (validWidgets.length > 0) {
      setWidgets(validWidgets)
    }
  }, [isTappWidgetsLoading, tappWidgets, rawLayoutData, ALL_AVAILABLE_WIDGETS])

  // 保存小组件配置到后端
  const handleWidgetsChange = async (newWidgets: WidgetConfig[]) => {
    // 过滤掉未注册的小组件（已丢失/删除的组件，包括 Tapp 小组件）
    const registeredWidgetIds = new Set(ALL_AVAILABLE_WIDGETS.map((w) => w.id))
    const validWidgets = newWidgets.filter((w) =>
      registeredWidgetIds.has(w.type),
    )

    setWidgets(validWidgets)

    // 只有管理员可以保存
    if (!isAdmin) return

    try {
      await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({
          layout: JSON.stringify(validWidgets), // 序列化为字符串存储
        }),
      })
    } catch (err) {
      console.error('保存小组件配置失败:', err)
    }
  }

  // 保存标题
  const handleTitleChange = async (newTitle: string) => {
    setDashboardTitle(newTitle)

    // 只有管理员可以保存
    if (!isAdmin) return

    try {
      await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({
          title: newTitle,
        }),
      })
    } catch (err) {
      console.error('保存标题失败:', err)
    }
  }

  // 监听自定义平台更新事件
  useEffect(() => {
    const handleCustomPlatformsUpdate = async (event: Event) => {
      const customEvent = event as CustomEvent<{ platforms: any[] }>
      // 只有管理员可以保存
      if (!isAdmin) return

      try {
        await fetch(`${API_URL}/api/config/dashboard`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-CSRF-Token': csrfToken,
          },
          credentials: 'include',
          body: JSON.stringify({
            custom_platforms: JSON.stringify(customEvent.detail.platforms),
          }),
        })
      } catch (err) {
        console.error('保存自定义平台失败:', err)
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
  }, [isAdmin, csrfToken])

  return (
    <AnimatedView className="min-h-screen lg:h-screen lg:overflow-hidden">
      <div className="h-full flex flex-col pt-20 pb-6 px-3 xs:px-4 sm:px-6">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col gap-4 p-2 relative min-h-0">
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
              {/* 背景标题 */}
              {isEditMode ? (
                <input
                  type="text"
                  aria-label="Dashboard Title"
                  value={dashboardTitle}
                  onChange={(e) => handleTitleChange(e.target.value)}
                  className="absolute left-1 whitespace-nowrap z-0 hidden md:block bg-transparent border-none outline-none p-0 m-0 w-full"
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
                  className="absolute left-1 whitespace-nowrap pointer-events-none z-0 hidden md:block"
                  style={{
                    top: `calc(30px - ${7.5 * titleFontSize}rem)`,
                    fontSize: `${6 * titleFontSize}rem`,
                    color: titleColorCss,
                    WebkitTextStroke: `0.5px color-mix(in srgb, ${titleColorCss} 30%, transparent)`,
                    fontFamily: currentFont.family,
                    fontWeight: 700,
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
                        <img
                          src={userInfo.avatar}
                          alt={userInfo.name}
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

                  {/* 编辑按钮 - 仅管理员可见，且仅在桌面端显示 */}
                  {isAdmin && (
                    <>
                      <div className="hidden lg:block h-6 w-px bg-gray-200 dark:bg-white/10 mx-1" />

                      {/* 字体选择器 - 仅在编辑模式下显示 */}
                      {isEditMode && (
                        <TitleFontSelector
                          csrfToken={csrfToken}
                          className="hidden lg:block"
                        />
                      )}

                      <button
                        onClick={() => setIsEditMode(!isEditMode)}
                        className={`
                          hidden lg:flex px-4 py-1.5 rounded-lg text-xs font-bold items-center gap-2 transition-all
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
                    </>
                  )}
                </motion.div>
              </motion.div>
            </div>
          </WidgetGrid>

          {/* Loading Overlay */}
          {isLoading && (
            <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/5 backdrop-blur-[1px] rounded-xl">
              <div className="animate-spin rounded-full h-12 w-12 border-4 border-gray-200/20 border-t-white/80"></div>
            </div>
          )}
        </div>
      </div>
    </AnimatedView>
  )
}
