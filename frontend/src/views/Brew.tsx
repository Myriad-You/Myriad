/**
 * Brew 阅读视图组件
 * RSS/Atom 订阅管理与阅读 - 重构版
 *
 * 性能优化：
 * - 接入统一动画调度器 (useBrewScheduler)
 * - useMemo 缓存导航项和计算结果
 * - useCallback 缓存所有回调函数
 * - useRef 避免闭包陷阱
 * - 子组件均已使用 React.memo 优化
 *
 * 权限控制：
 * - 游客可浏览所有内容（只读）
 * - 登录用户可编辑、收藏、标记已读等
 */

import type { SecondaryNavItem } from '../contexts/NavigationContext'

import type {
  AddSourceInput,
  BrewItem,
  BrewSource,
  BrewStats,
} from '../types/brew'
import { AnimatePresenceShim as AnimatePresence } from '@lib/motionShim'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  useMatch,
  useNavigate,
  useSearchParams,
} from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import BrewFeedList from '../components/brew/BrewFeedList'
import BrewReader from '../components/brew/BrewReader'
import BrewSourceGrid from '../components/brew/BrewSourceGrid'
import { BrewWallSkeleton } from '../components/brew/BrewTileWall'
import {
  BREW_MINE_CATEGORY,
  brewOwnItemPath,
  isOwnBrewSource,
} from '../components/brew/constants'
import { topicHue, topicNameKey } from '../components/brew/logic/topics'
import ControlIsland from '../components/brew/manager/ControlIsland'
import { isBrewTileGridEnabled } from '../components/brew/tileGridFlag'
import { Spinner } from '../components/Spinner'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useSecondaryNav } from '../contexts/NavigationContext'
import { useReadingListOptional } from '../contexts/ReadingListContext'
import { useBrewScheduler } from '../hooks/animation'
import { useBrewKeyboard } from '../hooks/useBrewKeyboard'
import { usePageSeo } from '../hooks/usePageSeo'
import * as brewApi from '../services/brewApi'
import {
  buildBrewItemPageSeo,
  buildBrewListPageSeo,
} from '../utils/brewPageSeo'
import {
  canAccessModuleVisibility,
  useModuleVisibilityPreferences,
} from '../utils/moduleVisibility'

// 导航图标
const NavIcons = {
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
  friends: (
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
        d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"
      />
    </svg>
  ),
  mine: (
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
        d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
      />
    </svg>
  ),
  starred: (
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
        d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z"
      />
    </svg>
  ),
}

// 预置分类 ID（用于前端逻辑判断）
const _PRESET_CATEGORY_IDS = {
  friends: 'friends',
  mine: 'mine',
} as const

type PresetCategoryId =
  (typeof _PRESET_CATEGORY_IDS)[keyof typeof _PRESET_CATEGORY_IDS]

// 预置分类的数据库存储值（后端使用的固定值，不要改动）
// 这些值与数据库中存储的分类名称一致（与 brew/constants 同源）
const PRESET_CATEGORY_DB_VALUES: Record<PresetCategoryId, string> = {
  friends: '友情链接',
  mine: BREW_MINE_CATEGORY,
}

// 需要合并展示文章的特殊分类（不显示网站卡片）

type CategoryKey = PresetCategoryId | 'all'

// 视图模式
// - sources: 显示网站卡片网格
// - items: 单个订阅源的文章列表
// - starred: 收藏文章列表
// - category-feed: 分类下所有文章的合并列表（特殊分类使用）
// - topic-feed: 一个主题下的跨源文章列表（主题磁贴进入）
type ViewMode =
  | 'sources'
  | 'items'
  | 'starred'
  | 'category-feed'
  | 'topic-feed'

/** Secondary-nav ids accepted via `/brew?category=` deep-link */
const BREW_CATEGORY_QUERY_IDS = new Set([
  'all',
  'friends',
  'mine',
  'starred',
] as const)

type BrewCategoryQueryId = 'all' | 'friends' | 'mine' | 'starred'

function isBrewCategoryQueryId(value: string): value is BrewCategoryQueryId {
  return BREW_CATEGORY_QUERY_IDS.has(value as BrewCategoryQueryId)
}

export default function Brew() {
  // 初始化动画调度器
  useBrewScheduler()
  const { t } = useI18n()
  const navigate = useNavigate()
  // 单路由 /brew/* 下解析文章 id，避免与 /brew 双 Route remount
  const itemMatch = useMatch('/brew/item/:itemId')
  const itemIdParam = itemMatch?.params.itemId
  const [searchParams, setSearchParams] = useSearchParams()
  const { preferences: moduleVisibility } = useModuleVisibilityPreferences()
  const moduleOpenToAll = canAccessModuleVisibility(
    moduleVisibility.modules.brew,
    { isAuthenticated: false, isAdmin: false },
  )
  /** 源列表是否至少加载过一次（用于区分「未知」与「非自有」；用 state 触发重算） */
  const [sourcesLoaded, setSourcesLoaded] = useState(false)

  // 获取预置分类的显示名称（国际化）
  const getCategoryName = useCallback(
    (categoryId: PresetCategoryId): string => {
      switch (categoryId) {
        case 'friends':
          return t.brew.friendLinks
        case 'mine':
          return t.brew.me
        default:
          return categoryId
      }
    },
    [t],
  )

  // 获取登录状态和管理员状态
  // - isAuthenticated: 用于已读状态等普通用户功能
  // - isAdmin: 用于添加、编辑、删除、刷新等管理功能
  const { isAuthenticated, isAdmin } = useAuth()

  // 阅读列表上下文 - 用于顺序阅读导航
  const readingList = useReadingListOptional()

  // 数据状态
  const [sources, setSources] = useState<BrewSource[]>([])
  const [items, setItems] = useState<BrewItem[]>([])
  const [stats, setStats] = useState<BrewStats | null>(null)
  const itemsRef = useRef<BrewItem[]>([]) // 用 ref 存储 items，供事件回调访问避免闭包陷阱

  // 同步 itemsRef，供事件回调中访问最新 items（避免闭包陷阱）
  useEffect(() => {
    itemsRef.current = items
  }, [items])

  // 视图状态
  const [viewMode, setViewMode] = useState<ViewMode>('sources')
  /** 当前主题（topic-feed）。key 用于过滤，nameKey 用于查 t.brew 的展示文案。 */
  const [selectedTopic, setSelectedTopic] = useState<{
    key: string
    nameKey: string
  } | null>(null)
  const [selectedSource, setSelectedSource] = useState<BrewSource | null>(null)
  const [selectedItem, setSelectedItem] = useState<BrewItem | null>(null)
  const [selectedCategory, setSelectedCategory] = useState<CategoryKey>('all')

  // UI 状态
  const [loading, setLoading] = useState(true)
  const [itemsLoading, setItemsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sourceRefreshing, setSourceRefreshing] = useState(false) // 订阅源刷新中状态

  // 收藏页面批量管理状态（仅登录用户可用）
  const [starredEditMode, setStarredEditMode] = useState(false)
  const [starredSelectedIds, setStarredSelectedIds] = useState<Set<number>>(
    new Set(),
  )
  const [starredProcessing, setStarredProcessing] = useState(false)

  // 分页状态
  const [_page, setPage] = useState(1)
  const [hasMore, setHasMore] = useState(true)
  const [total, setTotal] = useState(0)
  const pageRef = useRef(1) // 用 ref 存储 page，避免 loadItems 重新创建
  const loadRequestIdRef = useRef(0) // 请求版本号，用于取消过期请求

  // 当前打开文章对应的源（用于自有内容 SEO / 分享）
  const selectedItemSource = useMemo(() => {
    if (!selectedItem) return null
    return sources.find((s) => s.id === selectedItem.source_id) ?? null
  }, [selectedItem, sources])

  /**
   * 自有性三态：源未解析前为 unknown，禁止据此清掉 /brew/item/*。
   * - own: category 含「我」且非 admin_only
   * - not-own: 源已解析为非自有 / 网络搜索 / 源列表已加载但找不到源
   * - unknown: 仍在等源列表
   */
  const selectedItemOwnState: 'unknown' | 'own' | 'not-own' = useMemo(() => {
    if (!selectedItem) return 'unknown'
    if (selectedItem.fromWebSearch || selectedItem.source_id <= 0) {
      return 'not-own'
    }
    if (selectedItemSource) {
      return isOwnBrewSource(selectedItemSource) ? 'own' : 'not-own'
    }
    if (!sourcesLoaded) return 'unknown'
    // 源列表已加载但找不到该源 → 不按自有收录
    return 'not-own'
  }, [selectedItem, selectedItemSource, sourcesLoaded])

  const selectedItemIsOwn = selectedItemOwnState === 'own'

  // 列表 SEO；打开文章时仅自有内容用文章级 meta（非自有 noindex）
  usePageSeo(
    useMemo(() => {
      if (selectedItem && selectedItemOwnState !== 'unknown') {
        return buildBrewItemPageSeo({
          item: selectedItem,
          source: selectedItemSource,
          moduleOpenToAll,
        })
      }
      if (selectedItem && selectedItemOwnState === 'unknown') {
        // 解析中：先 noindex，避免误把外部文当可收录
        return {
          title: undefined,
          path: brewOwnItemPath(selectedItem.id),
          noindex: true,
        }
      }
      return buildBrewListPageSeo({
        listLabel: t.nav.brewReading || t.nav.brew,
        listDescription: t.widgets.brewDesc,
        moduleOpenToAll,
      })
    }, [
      selectedItem,
      selectedItemSource,
      selectedItemOwnState,
      moduleOpenToAll,
      t,
    ]),
  )

  // 自有文章：规范 URL 同步为 /brew/item/{id}
  // 仅在 own / not-own 确定后改 URL；unknown 时保持深链路径
  useEffect(() => {
    if (!selectedItem) return
    if (selectedItemOwnState === 'unknown') return
    if (selectedItemOwnState === 'own') {
      const want = String(selectedItem.id)
      if (itemIdParam !== want) {
        navigate(brewOwnItemPath(selectedItem.id), { replace: true })
      }
      return
    }
    // not-own：绝不保留 /brew/item/*，避免被爬取索引
    if (itemIdParam) {
      navigate('/brew', { replace: true })
    }
  }, [selectedItem, selectedItemOwnState, itemIdParam, navigate])

  // Deep-link: /brew/item/:itemId → 打开阅读器（非自有也可读，但不做 SEO）
  useEffect(() => {
    if (!itemIdParam) return
    const id = Number.parseInt(itemIdParam, 10)
    if (!Number.isFinite(id) || id <= 0) return
    if (selectedItem?.id === id) return

    let cancelled = false
    void (async () => {
      try {
        const item = await brewApi.getItem(id)
        if (cancelled) return
        setSelectedItem(item)
        // 补齐源列表后再判定 own/not-own（避免 unknown 被误清 URL）
        try {
          const all = await brewApi.getSources()
          if (cancelled) return
          setSourcesLoaded(true)
          setSources(all)
        } catch {
          /* loadSources 的常规路径仍会填 */
        }
      } catch (err) {
        console.error('[Brew] Failed to open deep-linked item:', err)
        if (!cancelled) navigate('/brew', { replace: true })
      }
    })()

    return () => {
      cancelled = true
    }
    // 仅在路由 itemId 变化时拉取；selectedItem 不进依赖以免循环
  }, [itemIdParam])

  // 计算 source_id -> theme_color 映射
  const sourceColors = useMemo(() => {
    const map = new Map<number, string>()
    sources.forEach((s) => {
      if (s.theme_color) {
        map.set(s.id, s.theme_color)
      }
    })
    return map
  }, [sources])

  // 构建二级导航项
  const navItems: SecondaryNavItem[] = useMemo(
    () => [
      {
        id: 'all',
        icon: NavIcons.all,
        label: t.brew.all,
        title: t.brew.all + t.brew.sources,
        ariaLabel: t.brew.all + t.brew.sources,
      },
      {
        id: 'friends',
        icon: NavIcons.friends,
        label: t.brew.friendLinks,
        title: t.brew.friendLinks,
        ariaLabel: t.brew.friendLinks,
      },
      {
        id: 'mine',
        icon: NavIcons.mine,
        label: t.brew.me,
        title: t.brew.me,
        ariaLabel: t.brew.me,
      },
      // 收藏是登录态功能：游客点进去主区恒为空（viewMode === 'starred' 还要
      // 求 isAuthenticated），所以入口本身就不该出现。
      ...(isAuthenticated
        ? [
            {
              id: 'starred',
              icon: NavIcons.starred,
              label: t.brew.starred,
              title: t.brew.starred,
              ariaLabel: t.brew.starred,
            },
          ]
        : []),
    ],
    [t, isAuthenticated],
  )

  // 使用二级导航 Hook
  const { activeId, setActiveId, setExpanded } = useSecondaryNav({
    routePath: '/brew',
    items: navItems,
    defaultActiveId: 'all',
    expandHint: t.brew.expandMenu,
  })

  // 监听展开事件（来自导航岛的自动展开请求）
  useEffect(() => {
    const handleExpandSecondary = (e: CustomEvent<{ path: string }>) => {
      if (e.detail.path === '/brew') {
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

  // 监听 Agent 打开文章事件
  useEffect(() => {
    const handleAgentOpenArticle = async (e: Event) => {
      const customEvent = e as CustomEvent<{
        articleId?: string
        articleLink?: string
        openLatest?: boolean
        webSearchArticle?: {
          id: number
          title: string
          author?: string
          sourceName?: string
          publishedAt?: string
          summary?: string
          relevanceReason?: string
          link?: string
          content?: string
          fromWebSearch?: boolean
          isWebSearchArticle?: boolean
        }
      }>
      console.log(
        '[Brew] Agent open article event received:',
        customEvent.detail,
      )
      console.log('[Brew] Current items count:', itemsRef.current.length)

      const { articleId, articleLink, openLatest, webSearchArticle } =
        customEvent.detail

      // 处理网络搜索文章：创建临时的 BrewItem
      if (webSearchArticle?.isWebSearchArticle) {
        console.log(
          '[Brew] Opening web search article:',
          webSearchArticle.title,
        )
        const tempBrewItem: BrewItem = {
          id: webSearchArticle.id,
          source_id: 0,
          source_name: webSearchArticle.sourceName || '网络搜索',
          source_icon: null,
          guid: `web_search_${webSearchArticle.id}`,
          title: webSearchArticle.title,
          link: webSearchArticle.link || '',
          summary:
            webSearchArticle.summary ||
            webSearchArticle.relevanceReason ||
            null,
          content: webSearchArticle.content || null, // 可能为空，BrewReader 会通过 API 获取
          image: null,
          audio_url: null,
          author: webSearchArticle.author || null,
          published_at: webSearchArticle.publishedAt
            ? new Date(webSearchArticle.publishedAt).getTime()
            : null,
          word_count: null,
          reading_time: null,
          is_read: false,
          is_starred: false,
          read_progress: null,
          created_at: Date.now(),
          fromWebSearch: true, // 标记为网络搜索文章
        }
        setSelectedItem(tempBrewItem)
        return
      }

      // 修复：优先使用后端返回的 articleId/articleLink，而不是无脑取第一篇
      // 这样才能正确打开"指定作者的最新文章"
      if (articleId || articleLink) {
        console.log('[Brew] Looking for specific article:', {
          articleId,
          articleLink,
        })

        // 如果 articleId 是数字，直接用 API 获取单篇文章（最可靠）
        const numericId = articleId
          ? Number.parseInt(articleId, 10)
          : Number.NaN
        if (!Number.isNaN(numericId)) {
          try {
            console.log('[Brew] Fetching article by ID:', numericId)
            const article = await brewApi.getItem(numericId)
            if (article) {
              console.log('[Brew] Got article by ID:', article.title)
              setSelectedItem(article)
              return
            }
          } catch (err) {
            console.warn('[Brew] Failed to fetch article by ID:', err)
            // 继续尝试其他方式
          }
        }

        // 先在已加载的 items 中查找
        // 优先用 link 匹配（最可靠），然后用 id 和 guid 匹配
        let targetItem = itemsRef.current.find((item) => {
          // 1. 优先匹配 link（最准确）
          if (articleLink && item.link === articleLink) return true
          // 2. 匹配数字 ID
          if (articleId && String(item.id) === articleId) return true
          // 3. 匹配 guid（后端可能返回 guid 作为 articleId）
          if (articleId && item.guid === articleId) return true
          return false
        })

        if (targetItem) {
          console.log('[Brew] Found article in loaded items:', targetItem.title)
          setSelectedItem(targetItem)
          return
        }

        // 如果没找到，尝试从 API 查找（只选中目标文，不把整页列表灌进 state）
        try {
          console.log('[Brew] Article not found locally, loading from API...')
          // 分页扫描，避免一次 per_page:100 的大包 + setItems 占内存
          const maxPages = 5
          const perPage = 20
          for (let page = 1; page <= maxPages; page++) {
            const data = await brewApi.getItems({
              per_page: perPage,
              page,
              filter: 'all',
            })
            console.log(
              '[Brew] Loaded page',
              page,
              'items:',
              data.items.length,
            )

            targetItem = data.items.find((item) => {
              if (articleLink && item.link === articleLink) return true
              if (articleId && String(item.id) === articleId) return true
              if (articleId && item.guid === articleId) return true
              return false
            })

            if (targetItem) {
              console.log('[Brew] Found article from API:', targetItem.title)
              // 仅打开阅读器，不替换当前列表 state（避免 100 条全文进堆）
              setSelectedItem(targetItem)
              return
            }

            // 已到末页
            if (data.items.length < perPage) break
            if (data.total > 0 && page * perPage >= data.total) break
          }

          console.warn(
            '[Brew] Article not found in API response. Looking for:',
            { articleId, articleLink },
          )
        } catch (err) {
          console.error('[Brew] Failed to load article for agent:', err)
        }
      }

      // 只有在没有指定 articleId/articleLink 且 openLatest 为 true 时，才取第一篇
      if (openLatest && !articleId && !articleLink) {
        console.log('[Brew] No specific article, opening latest...')
        // 如果 items 为空，先加载
        if (itemsRef.current.length === 0) {
          console.log('[Brew] Items empty, loading from API...')
          setItemsLoading(true)
          try {
            const data = await brewApi.getItems({
              per_page: 20,
              filter: 'all',
            })
            console.log('[Brew] Loaded items:', data.items.length)
            setItems(data.items)
            setTotal(data.total)
            // 打开第一篇
            if (data.items.length > 0) {
              console.log('[Brew] Opening first article:', data.items[0].title)
              setSelectedItem(data.items[0])
            }
          } catch (err) {
            console.error('[Brew] Failed to load items for agent:', err)
          } finally {
            setItemsLoading(false)
          }
        } else {
          // 直接打开第一篇
          console.log(
            '[Brew] Opening first existing article:',
            itemsRef.current[0].title,
          )
          setSelectedItem(itemsRef.current[0])
        }
      }
    }

    console.log('[Brew] Registering agent:open-brew-article event listener')
    window.addEventListener('agent:open-brew-article', handleAgentOpenArticle)

    // 检查是否有通过 sessionStorage 传递的待执行阅读列表（解决跨页面导航的竞态条件）
    const pendingReadingListStr = sessionStorage.getItem(
      'brew_pending_reading_list',
    )
    if (pendingReadingListStr) {
      try {
        const pendingAction = JSON.parse(pendingReadingListStr)
        console.log(
          '[Brew] Found pending reading list from sessionStorage:',
          pendingAction,
        )
        // 检查时间戳，只处理 10 秒内的请求
        if (
          pendingAction.timestamp &&
          Date.now() - pendingAction.timestamp < 10000
        ) {
          // 清除存储，避免重复执行
          sessionStorage.removeItem('brew_pending_reading_list')

          // 设置阅读列表
          if (pendingAction.readingList) {
            window.dispatchEvent(
              new CustomEvent('agent:set-reading-list', {
                detail: {
                  ...pendingAction.readingList,
                  createdAt: new Date(pendingAction.readingList.createdAt),
                },
              }),
            )
          }

          // 打开第一篇文章
          if (pendingAction.articleId) {
            setTimeout(() => {
              console.log(
                '[Brew] Opening first article from reading list:',
                pendingAction.articleId,
              )
              const syntheticEvent = new CustomEvent(
                'agent:open-brew-article',
                {
                  detail: {
                    articleId: pendingAction.articleId,
                    openLatest: false,
                    webSearchArticle: pendingAction.webSearchArticle, // 传递网络搜索文章数据
                  },
                },
              )
              handleAgentOpenArticle(syntheticEvent)
            }, 200)
          }
        } else {
          console.log('[Brew] Pending reading list expired, removing')
          sessionStorage.removeItem('brew_pending_reading_list')
        }
      } catch (e) {
        console.error('[Brew] Failed to parse pending reading list:', e)
        sessionStorage.removeItem('brew_pending_reading_list')
      }
    }

    // 检查是否有通过 sessionStorage 传递的待执行操作（解决跨页面导航的竞态条件）
    const pendingActionStr = sessionStorage.getItem('brew_pending_open_article')
    if (pendingActionStr) {
      try {
        const pendingAction = JSON.parse(pendingActionStr)
        console.log(
          '[Brew] Found pending action from sessionStorage:',
          pendingAction,
        )
        // 检查时间戳，只处理 10 秒内的请求
        if (
          pendingAction.timestamp &&
          Date.now() - pendingAction.timestamp < 10000
        ) {
          // 清除存储，避免重复执行
          sessionStorage.removeItem('brew_pending_open_article')
          // 模拟事件触发
          const syntheticEvent = new CustomEvent('agent:open-brew-article', {
            detail: pendingAction,
          })
          // 使用 setTimeout 确保组件完全渲染后再处理
          setTimeout(() => {
            console.log('[Brew] Executing pending action from sessionStorage')
            handleAgentOpenArticle(syntheticEvent)
          }, 100)
        } else {
          console.log('[Brew] Pending action expired, removing')
          sessionStorage.removeItem('brew_pending_open_article')
        }
      } catch (e) {
        console.error('[Brew] Failed to parse pending action:', e)
        sessionStorage.removeItem('brew_pending_open_article')
      }
    }

    return () => {
      console.log('[Brew] Unregistering agent:open-brew-article event listener')
      window.removeEventListener(
        'agent:open-brew-article',
        handleAgentOpenArticle,
      )
    }
  }, [])

  // 根据二级导航 id 同步视图模式 / 分类筛选
  const applyNavCategory = useCallback(
    (navId: string) => {
      if (navId === 'starred') {
        // 游客没有收藏：deep-link ?category=starred 回落到「全部」而不是空白主区
        if (!isAuthenticated) {
          setViewMode('sources')
          setSelectedCategory('all')
          setSelectedSource(null)
          return
        }
        setViewMode('starred')
        setSelectedSource(null)
      } else if (navId === 'friends') {
        setViewMode('sources')
        setSelectedCategory('friends')
        setSelectedSource(null)
      } else if (navId === 'mine') {
        // "我"分类特殊处理：直接展示合并的文章列表，不显示网站卡片
        setViewMode('category-feed')
        setSelectedCategory('mine')
        setSelectedSource(null)
      } else if (navId === 'all') {
        setViewMode('sources')
        setSelectedCategory('all')
        setSelectedSource(null)
      }
    },
    [isAuthenticated],
  )

  // 监听导航变化
  // 用于追踪上一次的 activeId，避免 viewMode 变化导致重复执行
  const prevActiveIdRef = useRef(activeId)

  // 监听导航变化 - 只在 activeId 真正变化时执行
  useEffect(() => {
    // 如果 activeId 没有变化，不执行任何操作
    if (prevActiveIdRef.current === activeId) {
      return
    }
    prevActiveIdRef.current = activeId
    applyNavCategory(activeId)
  }, [activeId, applyNavCategory])

  // Deep-link: /brew?category=friends|mine|all|starred
  // Must apply category state even when activeId already matches (prevActiveIdRef
  // would early-return), then consume the query so refresh/back stays clean.
  useEffect(() => {
    const categoryParam = searchParams.get('category')
    if (!categoryParam || !isBrewCategoryQueryId(categoryParam)) return

    applyNavCategory(categoryParam)
    prevActiveIdRef.current = categoryParam
    setActiveId(categoryParam)

    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        next.delete('category')
        return next
      },
      { replace: true },
    )
  }, [searchParams, applyNavCategory, setActiveId, setSearchParams])

  // 加载订阅源列表
  const loadSources = useCallback(async () => {
    try {
      const data = await brewApi.getSources()
      setSourcesLoaded(true)
      setSources(data)
    } catch (err) {
      console.error('Failed to load sources:', err)
      setError('加载订阅源失败')
    }
  }, [])

  // 加载统计信息
  const loadStats = useCallback(async () => {
    try {
      const data = await brewApi.getStats()
      setStats(data)
    } catch (err) {
      console.error('Failed to load stats:', err)
    }
  }, [])

  // 加载文章列表 - 接受参数以避免闭包问题
  const loadItems = useCallback(
    async (
      reset = false,
      sourceId?: number,
      mode?: ViewMode,
      categoryFilter?: string,
      topicFilter?: string,
    ) => {
      // 生成新的请求 ID，用于防止竞态条件
      const requestId = ++loadRequestIdRef.current

      setItemsLoading(true)
      try {
        const currentPage = reset ? 1 : pageRef.current
        const filter = mode === 'starred' ? 'starred' : 'all'

        const data = await brewApi.getItems({
          source_id: sourceId || undefined,
          category: categoryFilter || undefined,
          // 主题过滤：`topic IS NULL` 的文章天然落空，打标是离线的
          topic: topicFilter || undefined,
          filter,
          page: currentPage,
          per_page: 20,
        })

        // 检查是否是最新的请求，忽略过期请求的响应
        if (requestId !== loadRequestIdRef.current) {
          return
        }

        if (reset) {
          setItems(data.items)
          setPage(1)
          pageRef.current = 1
        } else {
          setItems((prev) => {
            const existingIds = new Set(prev.map((item) => item.id))
            const newItems = data.items.filter(
              (item) => !existingIds.has(item.id),
            )
            return [...prev, ...newItems]
          })
        }

        setTotal(data.total)
        // Prefer server total — `items.length >= per_page` is wrong when total
        // is an exact multiple of the page size (would keep requesting empty pages).
        const perPage = data.per_page > 0 ? data.per_page : 20
        if (typeof data.total === 'number' && data.total >= 0) {
          setHasMore(currentPage * perPage < data.total)
        } else {
          setHasMore(data.items.length >= perPage)
        }
      } catch (err) {
        // 忽略过期请求的错误
        if (requestId !== loadRequestIdRef.current) {
          return
        }
        console.error('Failed to load items:', err)
        setError('加载文章失败')
      } finally {
        // 只有最新请求才更新 loading 状态
        if (requestId === loadRequestIdRef.current) {
          setItemsLoading(false)
        }
      }
    },
    [],
  ) // 移除 page 依赖，使用 ref

  // 初始加载
  useEffect(() => {
    const init = async () => {
      setLoading(true)
      await Promise.all([loadSources(), loadStats()])
      setLoading(false)
    }
    init()
  }, [loadSources, loadStats])

  // 登录用户：订阅 Brew WS（新文章/源错误等），避免 createBrewWebSocket 零调用
  const brewWsRefreshRef = useRef<() => void>(() => {})
  brewWsRefreshRef.current = () => {
    void loadSources()
    void loadStats()
  }
  useEffect(() => {
    if (!isAuthenticated) return
    let closed = false
    let ws: WebSocket | null = null
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null

    const connect = () => {
      if (closed) return
      try {
        ws = brewApi.createBrewWebSocket(
          (notification) => {
            console.debug('[Brew] WS notification', notification)
            brewWsRefreshRef.current()
          },
          () => {
            if (closed) return
            reconnectTimer = setTimeout(connect, 5000)
          },
        )
        ws.onclose = () => {
          if (closed) return
          reconnectTimer = setTimeout(connect, 5000)
        }
      } catch (err) {
        console.warn('[Brew] WS connect failed', err)
        reconnectTimer = setTimeout(connect, 8000)
      }
    }
    connect()

    return () => {
      closed = true
      if (reconnectTimer) clearTimeout(reconnectTimer)
      try {
        ws?.close()
      } catch {
        /* ignore */
      }
    }
  }, [isAuthenticated])

  // 当进入文章视图时加载文章
  useEffect(() => {
    if (viewMode === 'items' && selectedSource) {
      loadItems(true, selectedSource.id, viewMode)
    } else if (viewMode === 'starred') {
      loadItems(true, undefined, viewMode)
    } else if (viewMode === 'category-feed' && selectedCategory !== 'all') {
      // 分类合并文章视图：加载该分类下所有文章
      const categoryDbValue =
        PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]
      loadItems(true, undefined, viewMode, categoryDbValue)
    } else if (viewMode === 'topic-feed' && selectedTopic) {
      // 主题跨源列表：只按 topic 过滤，不限源
      loadItems(true, undefined, viewMode, undefined, selectedTopic.key)
    }
  }, [viewMode, selectedSource, selectedCategory, selectedTopic, loadItems])

  // 处理源点击 - 进入该源的文章列表
  const handleSourceClick = useCallback((source: BrewSource) => {
    // 立即清空旧文章列表，避免切换时显示上一个源的内容
    setItems([])
    setPage(1)
    pageRef.current = 1
    setHasMore(true)
    setTotal(0)
    // 设置新源和视图模式
    setSelectedSource(source)
    setViewMode('items')
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.BREW_OPEN_SOURCE, {
          target: source.id,
          throttleMs: 2000,
        })
      },
    )
  }, [])

  // 处理订阅源更新（如卡片尺寸变更）
  const handleSourceUpdate = (updatedSource: BrewSource) => {
    setSources((prev) =>
      prev.map((s) => (s.id === updatedSource.id ? updatedSource : s)),
    )
  }

  // 处理返回源列表
  const handleBackToSources = useCallback(() => {
    setSelectedSource(null)
    setSelectedItem(null)
    setViewMode('sources')
    setActiveId(selectedCategory)
  }, [selectedCategory, setActiveId])

  // 处理从分类合并文章视图返回
  const handleBackFromCategoryFeed = useCallback(() => {
    setSelectedItem(null)
    setViewMode('sources')
    setSelectedCategory('all')
    setActiveId('all')
  }, [setActiveId])

  /** 主题磁贴 → 跨源列表 */
  const handleTopicClick = useCallback(
    (topicKey: string, topicNameKey: string) => {
      setSelectedTopic({ key: topicKey, nameKey: topicNameKey })
      setSelectedSource(null)
      setViewMode('topic-feed')
    },
    [],
  )

  const handleBackFromTopicFeed = useCallback(() => {
    setSelectedItem(null)
    setSelectedTopic(null)
    setViewMode('sources')
  }, [])

  /**
   * Deep-link: `/brew?source=<id>` 与 `/brew?topic=<key>`。
   *
   * 首页磁贴点击落到这里 —— 首页不打开阅读器，只把「看哪个源 / 哪个主题」
   * 交给 Brew 页。`source=` 需要等 sources 加载完才能拿到对象。
   */
  useEffect(() => {
    const topicParam = searchParams.get('topic')
    if (topicParam) {
      const nameKey = topicNameKey(topicParam)
      if (nameKey) {
        handleTopicClick(topicParam, nameKey)
      }
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev)
          next.delete('topic')
          return next
        },
        { replace: true },
      )
      return
    }

    const sourceParam = searchParams.get('source')
    if (!sourceParam) return
    const id = Number(sourceParam)
    if (!Number.isFinite(id)) return
    // sources 还没到就先留着 query，下一轮再试
    const target = sources.find((x) => x.id === id)
    if (!target) return

    handleSourceClick(target)
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        next.delete('source')
        return next
      },
      { replace: true },
    )
  }, [
    searchParams,
    sources,
    handleTopicClick,
    handleSourceClick,
    setSearchParams,
  ])

  // 处理文章选择
  const handleItemSelect = async (item: BrewItem) => {
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.BREW_OPEN_ITEM, {
          target: item.source_id || item.id,
          throttleMs: 1500,
        })
      },
    )
    // 如果未读，自动标记为已读。
    // 游客侧 is_read 恒为 false（后端 LEFT JOIN user_id = -1），markRead 必 401，
    // 所以未登录时既不写库也不动本地已读态。
    if (!isAuthenticated) {
      setSelectedItem(item)
      return
    }
    if (!item.is_read) {
      // 先更新为已读状态再显示
      const updatedItem = { ...item, is_read: true }
      setSelectedItem(updatedItem)

      try {
        await brewApi.markRead(item.id)
        setItems((prev) =>
          prev.map((i) => (i.id === item.id ? { ...i, is_read: true } : i)),
        )
        setStats((prev) =>
          prev ? { ...prev, total_unread: prev.total_unread - 1 } : prev,
        )
        // 同时更新 sources 的 unread_count 和 recent_items 中对应文章的 is_read
        setSources((prev) =>
          prev.map((s) =>
            s.id === item.source_id
              ? {
                  ...s,
                  unread_count: s.unread_count - 1,
                  recent_items: s.recent_items?.map((ri) =>
                    ri.id === item.id ? { ...ri, is_read: true } : ri,
                  ),
                }
              : s,
          ),
        )
      } catch (err) {
        console.error('Failed to mark as read:', err)
        // API 失败时恢复状态
        setSelectedItem(item)
      }
    } else {
      setSelectedItem(item)
    }
  }

  // 阅读列表导航 - 根据文章 ID 跳转
  const handleNavigateToArticle = useCallback(
    async (articleId: number) => {
      // 首先检查阅读列表中是否有这篇文章（可能是网络搜索结果）
      if (readingList?.currentList) {
        const listItem = readingList.currentList.items.find(
          (i) => i.id === articleId,
        )
        if (listItem?.fromWebSearch) {
          console.log(
            '[Brew] Navigating to web search article from reading list:',
            listItem.title,
          )
          const tempBrewItem: BrewItem = {
            id: listItem.id,
            source_id: 0,
            source_name: listItem.sourceName || '网络搜索',
            source_icon: null,
            guid: `web_search_${listItem.id}`,
            title: listItem.title,
            link: listItem.link || '',
            summary: listItem.summary || listItem.relevanceReason || null,
            content: listItem.content || null, // 可能为空，BrewReader 会通过 API 获取
            image: null,
            audio_url: null,
            author: listItem.author || null,
            published_at: listItem.publishedAt
              ? new Date(listItem.publishedAt).getTime()
              : null,
            word_count: null,
            reading_time: null,
            is_read: false,
            is_starred: false,
            read_progress: null,
            created_at: Date.now(),
            fromWebSearch: true,
          }
          setSelectedItem(tempBrewItem)
          // 更新阅读列表的当前位置
          const itemIndex = readingList.currentList.items.findIndex(
            (i) => i.id === articleId,
          )
          if (itemIndex !== -1) {
            readingList.goToArticle(itemIndex)
          }
          return
        }
      }

      // 先尝试从当前 items 列表中查找
      let targetItem = items.find((i) => i.id === articleId)

      if (!targetItem) {
        // 如果在当前列表中找不到，从 API 获取
        try {
          targetItem = await brewApi.getItem(articleId)
        } catch (err) {
          console.error('Failed to fetch article:', err)
          return
        }
      }

      if (targetItem) {
        handleItemSelect(targetItem)
      }
    },
    [items, handleItemSelect, readingList],
  )

  // 处理收藏切换
  const handleToggleStar = async (item: BrewItem) => {
    try {
      if (item.is_starred) {
        await brewApi.unstarItem(item.id)
        void import('../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.BREW_UNSTAR, {
              target: item.source_id || item.id,
              throttleMs: 1000,
            })
          },
        )
      } else {
        await brewApi.starItem(item.id)
        void import('../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.BREW_STAR, {
              target: item.source_id || item.id,
              throttleMs: 1000,
            })
          },
        )
      }

      setItems((prev) =>
        prev.map((i) =>
          i.id === item.id ? { ...i, is_starred: !i.is_starred } : i,
        ),
      )

      if (selectedItem?.id === item.id) {
        setSelectedItem((prev) =>
          prev ? { ...prev, is_starred: !prev.is_starred } : prev,
        )
      }

      setStats((prev) => {
        if (!prev) return prev
        return {
          ...prev,
          total_starred: item.is_starred
            ? prev.total_starred - 1
            : prev.total_starred + 1,
        }
      })
    } catch (err) {
      console.error('Failed to toggle star:', err)
    }
  }

  // 处理添加订阅源
  const handleAddSource = async ({
    url,
    name,
    category,
    icon,
    sourceType,
    feedType,
    notionToken,
  }: AddSourceInput) => {
    const source = await brewApi.addSource({
      url,
      name,
      category,
      source_type: sourceType,
      feed_type: feedType,
      extra_config: notionToken ? { token: notionToken } : undefined,
    })
    // 如果有自定义图标，添加后立即更新
    if (icon && source.id) {
      const updatedSource = await brewApi.updateSource(source.id, { icon })
      setSources((prev) => [...prev, updatedSource])
    } else {
      setSources((prev) => [...prev, source])
    }
    loadStats()
  }

  // 处理刷新订阅源
  const handleRefreshSource = async (sourceId: number) => {
    setSourceRefreshing(true)
    try {
      const newCount = await brewApi.refreshSource(sourceId)
      if (newCount > 0) {
        if (viewMode === 'items' && selectedSource?.id === sourceId) {
          loadItems(true, sourceId, viewMode)
        }
        loadStats()
        loadSources()
      }
    } catch (err) {
      console.error('Failed to refresh source:', err)
    } finally {
      setSourceRefreshing(false)
    }
  }

  // 处理全部标记已读
  const handleMarkAllRead = async () => {
    try {
      // 分类合并文章视图时，按分类标记已读
      const categoryFilter =
        viewMode === 'category-feed' && selectedCategory !== 'all'
          ? PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]
          : undefined

      const marked = await brewApi.markAllRead({
        source_id: selectedSource?.id || undefined,
        category: categoryFilter,
      })

      if (marked > 0) {
        // 只更新当前列表中文章的已读状态，不重新加载列表，避免破坏排序
        setItems((prev) => prev.map((item) => ({ ...item, is_read: true })))
        // 更新订阅源的未读计数
        if (selectedSource) {
          setSources((prev) =>
            prev.map((s) =>
              s.id === selectedSource.id ? { ...s, unread_count: 0 } : s,
            ),
          )
        } else if (categoryFilter) {
          // 分类视图：更新该分类下所有源的未读计数
          setSources((prev) =>
            prev.map((s) => {
              if (!s.category) return s
              const cats = s.category.split(',').map((c) => c.trim())
              if (cats.includes(categoryFilter)) {
                return { ...s, unread_count: 0 }
              }
              return s
            }),
          )
        }
        loadStats()
      }
    } catch (err) {
      console.error('Failed to mark all read:', err)
    }
  }

  // 收藏页面批量管理
  const handleStarredEnterEditMode = useCallback(() => {
    setStarredEditMode(true)
    setStarredSelectedIds(new Set())
  }, [])

  const handleStarredExitEditMode = useCallback(() => {
    setStarredEditMode(false)
    setStarredSelectedIds(new Set())
  }, [])

  const handleStarredSelectAll = useCallback(() => {
    if (starredSelectedIds.size === items.length) {
      setStarredSelectedIds(new Set())
    } else {
      setStarredSelectedIds(new Set(items.map((i) => i.id)))
    }
  }, [items, starredSelectedIds.size])

  const handleStarredBatchUnstar = useCallback(async () => {
    if (starredSelectedIds.size === 0) return

    setStarredProcessing(true)
    try {
      // 批量取消收藏
      const promises = Array.from(starredSelectedIds).map((id) =>
        brewApi.unstarItem(id),
      )
      await Promise.all(promises)

      // 更新列表
      setItems((prev) => prev.filter((i) => !starredSelectedIds.has(i.id)))
      setTotal((prev) => prev - starredSelectedIds.size)

      // 更新统计
      setStats((prev) =>
        prev
          ? {
              ...prev,
              total_starred: prev.total_starred - starredSelectedIds.size,
            }
          : prev,
      )

      // 退出编辑模式
      handleStarredExitEditMode()
    } catch (err) {
      console.error('Failed to batch unstar:', err)
    } finally {
      setStarredProcessing(false)
    }
  }, [starredSelectedIds, handleStarredExitEditMode])

  const handleStarredBack = useCallback(() => {
    setViewMode('sources')
    setActiveId('all')
    handleStarredExitEditMode()
  }, [setActiveId, handleStarredExitEditMode])

  // 加载更多 - useCallback 缓存
  const handleLoadMore = useCallback(() => {
    if (!itemsLoading && hasMore) {
      pageRef.current += 1
      setPage(pageRef.current)
      // 分类合并文章视图需要传递分类筛选
      const categoryFilter =
        viewMode === 'category-feed' && selectedCategory !== 'all'
          ? PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]
          : undefined
      loadItems(false, selectedSource?.id, viewMode, categoryFilter)
    }
  }, [
    itemsLoading,
    hasMore,
    selectedSource?.id,
    viewMode,
    selectedCategory,
    loadItems,
  ])

  // 关闭阅读器 - useCallback 缓存
  const handleCloseReader = useCallback(() => {
    setSelectedItem(null)
    if (itemIdParam) {
      navigate('/brew', { replace: true })
    }
  }, [itemIdParam, navigate])

  // 处理已读/未读切换
  const handleToggleRead = async (item: BrewItem) => {
    // 已读态是登录态数据；游客按 `m` 只会拿到 401
    if (!isAuthenticated) return
    try {
      if (item.is_read) {
        await brewApi.markUnread(item.id)
      } else {
        await brewApi.markRead(item.id)
      }

      const newReadState = !item.is_read

      setItems((prev) =>
        prev.map((i) =>
          i.id === item.id ? { ...i, is_read: newReadState } : i,
        ),
      )

      if (selectedItem?.id === item.id) {
        setSelectedItem((prev) =>
          prev ? { ...prev, is_read: newReadState } : prev,
        )
      }

      setStats((prev) => {
        if (!prev) return prev
        return {
          ...prev,
          total_unread: newReadState
            ? prev.total_unread - 1
            : prev.total_unread + 1,
        }
      })

      // 同时更新 sources 的 unread_count 和 recent_items 中对应文章的 is_read
      setSources((prev) =>
        prev.map((s) =>
          s.id === item.source_id
            ? {
                ...s,
                unread_count: newReadState
                  ? s.unread_count - 1
                  : s.unread_count + 1,
                recent_items: s.recent_items?.map((ri) =>
                  ri.id === item.id ? { ...ri, is_read: newReadState } : ri,
                ),
              }
            : s,
        ),
      )
    } catch (err) {
      console.error('Failed to toggle read:', err)
    }
  }

  // 键盘快捷键
  useBrewKeyboard({
    items,
    selectedItem,
    enabled: true,
    onSelectItem: (item) =>
      item ? handleItemSelect(item) : handleCloseReader(),
    onToggleRead: handleToggleRead,
    onToggleStar: handleToggleStar,
    onRefresh: () => selectedSource && handleRefreshSource(selectedSource.id),
    onAddSource: () => {}, // 添加功能已整合到首页底部操作栏
    onMarkAllRead: handleMarkAllRead,
    onCloseReader: handleCloseReader,
    onShowHelp: () => {}, // 快捷键帮助已整合到首页底部操作栏
  })

  if (loading) {
    // 二次进入：先按上次的装箱结果铺一层骨架，避免整屏重排。没有缓存
    // （首次访问 / flag 关）时 BrewWallSkeleton 返回 null，回落到 spinner。
    // 与 BrewSourceGrid 传给磁贴墙的 scope 保持同一口径（分类 + 排序模式），
    // 否则读的是另一把缓存键，骨架和真实布局对不上
    const skeletonScope =
      selectedCategory === 'all'
        ? 'all'
        : PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]
    const skeleton =
      viewMode === 'sources' && isBrewTileGridEnabled() ? (
        <BrewWallSkeleton scope={`${skeletonScope}:smart`} />
      ) : null

    return (
      <AnimatedView className="min-h-screen">
        <div className="h-full flex flex-col pt-20 pb-28 sm:pb-24 md:pb-12 px-3 xs:px-4 sm:px-6">
          <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col relative min-h-0">
            {skeleton ?? (
              <div className="flex flex-1 items-center justify-center">
                <Spinner size="lg" className="text-orange-500" />
              </div>
            )}
          </div>
        </div>
      </AnimatedView>
    )
  }

  return (
    <AnimatedView className="min-h-screen">
      <div className="h-full flex flex-col pt-20 pb-28 sm:pb-24 md:pb-12 px-3 xs:px-4 sm:px-6">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col relative min-h-0">
          {/* 源列表视图 */}
          {viewMode === 'sources' && (
            <BrewSourceGrid
              sources={sources}
              category={
                selectedCategory === 'all'
                  ? undefined
                  : PRESET_CATEGORY_DB_VALUES[
                      selectedCategory as PresetCategoryId
                    ]
              }
              onSourceClick={handleSourceClick}
              onRefreshSource={handleRefreshSource}
              onSourceUpdate={handleSourceUpdate}
              onSourcesChange={() => {
                loadSources()
                loadStats()
              }}
              onAddSource={handleAddSource}
              onTopicClick={handleTopicClick}
              isAuthenticated={isAuthenticated}
              isAdmin={isAdmin}
            />
          )}

          {/* 文章列表视图 */}
          {viewMode === 'items' && selectedSource && (
            <div className="relative pb-24 sm:pb-16">
              {/* 控制岛 - 文章列表模式 */}
              <ControlIsland
                sources={sources}
                filteredSources={sources}
                categories={[]}
                isAdmin={isAdmin}
                isAuthenticated={isAuthenticated}
                feedMode={{
                  source: selectedSource,
                  total,
                  onBack: handleBackToSources,
                  onRefresh: () => handleRefreshSource(selectedSource.id),
                  onMarkAllRead: handleMarkAllRead,
                  isRefreshing: sourceRefreshing,
                }}
              />

              {/* 文章列表 */}
              <BrewFeedList
                items={items}
                selectedItem={selectedItem}
                loading={itemsLoading}
                hasMore={hasMore}
                total={total}
                onItemSelect={handleItemSelect}
                onToggleStar={handleToggleStar}
                onLoadMore={handleLoadMore}
                sourceColors={sourceColors}
                isAuthenticated={isAuthenticated}
              />
            </div>
          )}

          {/* 分类合并文章视图 - 用于"我"等特殊分类 */}
          {viewMode === 'category-feed' && selectedCategory !== 'all' && (
            <div className="relative pb-24 sm:pb-16">
              {/* 控制岛 - 分类合并文章列表模式 */}
              <ControlIsland
                sources={sources}
                filteredSources={sources}
                categories={[]}
                isAdmin={isAdmin}
                isAuthenticated={isAuthenticated}
                categoryFeedMode={{
                  categoryName:
                    PRESET_CATEGORY_DB_VALUES[
                      selectedCategory as PresetCategoryId
                    ],
                  categoryLabel: getCategoryName(
                    selectedCategory as PresetCategoryId,
                  ),
                  total,
                  unreadCount: sources
                    .filter((s) => {
                      const targetCat =
                        PRESET_CATEGORY_DB_VALUES[
                          selectedCategory as PresetCategoryId
                        ]
                      if (!s.category) return false
                      return s.category
                        .split(',')
                        .map((c) => c.trim())
                        .includes(targetCat)
                    })
                    .reduce((sum, s) => sum + s.unread_count, 0),
                  onBack: handleBackFromCategoryFeed,
                  onMarkAllRead: handleMarkAllRead,
                }}
              />

              {/* 文章列表 */}
              <BrewFeedList
                items={items}
                selectedItem={selectedItem}
                loading={itemsLoading}
                hasMore={hasMore}
                total={total}
                onItemSelect={handleItemSelect}
                onToggleStar={handleToggleStar}
                onLoadMore={handleLoadMore}
                sourceColors={sourceColors}
                isAuthenticated={isAuthenticated}
              />
            </div>
          )}

          {/* 主题跨源文章视图 - 主题磁贴进入 */}
          {viewMode === 'topic-feed' && selectedTopic && (
            <div className="relative pb-24 sm:pb-16">
              <ControlIsland
                sources={sources}
                filteredSources={sources}
                categories={[]}
                isAdmin={isAdmin}
                isAuthenticated={isAuthenticated}
                topicFeedMode={{
                  topicKey: selectedTopic.key,
                  // 主题名走 i18n，跟界面语言；不在组件里写死中文
                  topicLabel:
                    (t.brew as unknown as Record<string, string | undefined>)[
                      selectedTopic.nameKey
                    ] ?? selectedTopic.key,
                  total,
                  onBack: handleBackFromTopicFeed,
                }}
                topicHue={topicHue(selectedTopic.key) ?? undefined}
              />

              <BrewFeedList
                items={items}
                selectedItem={selectedItem}
                loading={itemsLoading}
                hasMore={hasMore}
                total={total}
                onItemSelect={handleItemSelect}
                onToggleStar={handleToggleStar}
                onLoadMore={handleLoadMore}
                sourceColors={sourceColors}
                isAuthenticated={isAuthenticated}
              />
            </div>
          )}

          {/* 收藏文章视图 - 仅登录用户可用 */}
          {viewMode === 'starred' && isAuthenticated && (
            <div className="relative pb-24 sm:pb-16">
              {/* 控制岛 - 收藏模式 */}
              <ControlIsland
                sources={sources}
                filteredSources={sources}
                categories={[]}
                starredMode={{
                  total: stats?.total_starred || 0,
                  selectedIds: starredSelectedIds,
                  isEditMode: starredEditMode,
                  onBack: handleStarredBack,
                  onEnterEditMode: handleStarredEnterEditMode,
                  onExitEditMode: handleStarredExitEditMode,
                  onSelectAll: handleStarredSelectAll,
                  onBatchUnstar: handleStarredBatchUnstar,
                  isProcessing: starredProcessing,
                }}
              />

              <BrewFeedList
                items={items}
                selectedItem={selectedItem}
                loading={itemsLoading}
                hasMore={hasMore}
                total={total}
                onItemSelect={handleItemSelect}
                onToggleStar={handleToggleStar}
                onLoadMore={handleLoadMore}
                sourceColors={sourceColors}
                editMode={starredEditMode}
                selectedIds={starredSelectedIds}
                onItemSelectToggle={(id) => {
                  setStarredSelectedIds((prev) => {
                    const newSet = new Set(prev)
                    if (newSet.has(id)) {
                      newSet.delete(id)
                    } else {
                      newSet.add(id)
                    }
                    return newSet
                  })
                }}
              />
            </div>
          )}

          {/* 阅读器 */}
          <AnimatePresence mode="wait">
            {selectedItem && (
              <BrewReader
                key="brew-reader"
                item={selectedItem}
                onClose={handleCloseReader}
                onToggleStar={() => handleToggleStar(selectedItem)}
                isAuthenticated={isAuthenticated}
                isAdmin={isAdmin}
                sourceType={selectedItemSource?.source_type}
                // 仅自有文章用站内规范 URL 分享；外部订阅仍复制原文链接
                shareUrl={
                  selectedItemIsOwn
                    ? `${typeof window !== 'undefined' ? window.location.origin : ''}${brewOwnItemPath(selectedItem.id)}`
                    : undefined
                }
                onNavigateToArticle={handleNavigateToArticle}
                articleList={readingList?.currentList ? undefined : items}
                currentArticleIndex={
                  readingList?.currentList
                    ? undefined
                    : items.findIndex((i) => i.id === selectedItem.id)
                }
              />
            )}
          </AnimatePresence>

          {/* 错误提示 */}
          {error && (
            <div className="fixed bottom-4 right-4 bg-red-500 text-white px-4 py-2 rounded-lg shadow-lg z-50">
              {error}
              <button
                className="ml-2 hover:underline"
                onClick={() => setError(null)}
              >
                关闭
              </button>
            </div>
          )}
        </div>
      </div>
    </AnimatedView>
  )
}
