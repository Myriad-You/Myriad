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

import { useEffect, useState, useCallback, useMemo, useRef } from 'react';
import { AnimatePresence } from 'framer-motion';
import AnimatedView from '../components/AnimatedView';
import BrewSourceGrid from '../components/brew/BrewSourceGrid';
import BrewFeedList from '../components/brew/BrewFeedList';
import BrewReader from '../components/brew/BrewReader';
import ControlIsland from '../components/brew/manager/ControlIsland';
import { useBrewKeyboard } from '../hooks/useBrewKeyboard';
import { useSecondaryNav, type SecondaryNavItem } from '../contexts/NavigationContext';
import { useI18n } from '../contexts/I18nContext';
import { useBrewScheduler, useBrewAnimationConfig } from '../hooks/animation/pages/brew';
import { useAuth } from '../contexts/AuthContext';
import type { BrewSource, BrewItem, BrewStats } from '../types/brew';
import * as brewApi from '../services/brewApi';

// 导航图标
const NavIcons = {
  all: (
    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z" />
    </svg>
  ),
  friends: (
    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
    </svg>
  ),
  mine: (
    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
    </svg>
  ),
  starred: (
    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z" />
    </svg>
  ),
};

// 预置分类 ID（用于前端逻辑判断）
const PRESET_CATEGORY_IDS = {
  friends: 'friends',
  mine: 'mine',
} as const;

type PresetCategoryId = typeof PRESET_CATEGORY_IDS[keyof typeof PRESET_CATEGORY_IDS];

// 预置分类的数据库存储值（后端使用的固定值，不要改动）
// 这些值与数据库中存储的分类名称一致
const PRESET_CATEGORY_DB_VALUES: Record<PresetCategoryId, string> = {
  friends: '友情链接',
  mine: '我',
};

// 需要合并展示文章的特殊分类（不显示网站卡片）
const MERGED_FEED_CATEGORIES: PresetCategoryId[] = ['mine'];

type CategoryKey = PresetCategoryId | 'all';

// 视图模式
// - sources: 显示网站卡片网格
// - items: 单个订阅源的文章列表
// - starred: 收藏文章列表
// - category-feed: 分类下所有文章的合并列表（特殊分类使用）
type ViewMode = 'sources' | 'items' | 'starred' | 'category-feed';

export default function Brew() {
  // 初始化动画调度器
  useBrewScheduler();
  const animConfig = useBrewAnimationConfig();
  const { t } = useI18n();

  // 获取预置分类的显示名称（国际化）
  const getCategoryName = useCallback((categoryId: PresetCategoryId): string => {
    switch (categoryId) {
      case 'friends': return t.brew.friendLinks;
      case 'mine': return t.brew.me;
      default: return categoryId;
    }
  }, [t]);

  // 获取登录状态和管理员状态
  // - isAuthenticated: 用于已读状态等普通用户功能
  // - isAdmin: 用于添加、编辑、删除、刷新等管理功能
  const { isAuthenticated, isAdmin } = useAuth();

  // 数据状态
  const [sources, setSources] = useState<BrewSource[]>([]);
  const [items, setItems] = useState<BrewItem[]>([]);
  const [stats, setStats] = useState<BrewStats | null>(null);

  // 视图状态
  const [viewMode, setViewMode] = useState<ViewMode>('sources');
  const [selectedSource, setSelectedSource] = useState<BrewSource | null>(null);
  const [selectedItem, setSelectedItem] = useState<BrewItem | null>(null);
  const [selectedCategory, setSelectedCategory] = useState<CategoryKey>('all');

  // UI 状态
  const [loading, setLoading] = useState(true);
  const [itemsLoading, setItemsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sourceRefreshing, setSourceRefreshing] = useState(false);  // 订阅源刷新中状态

  // 收藏页面批量管理状态（仅登录用户可用）
  const [starredEditMode, setStarredEditMode] = useState(false);
  const [starredSelectedIds, setStarredSelectedIds] = useState<Set<number>>(new Set());
  const [starredProcessing, setStarredProcessing] = useState(false);

  // 分页状态
  const [page, setPage] = useState(1);
  const [hasMore, setHasMore] = useState(true);
  const [total, setTotal] = useState(0);
  const pageRef = useRef(1);  // 用 ref 存储 page，避免 loadItems 重新创建
  const loadRequestIdRef = useRef(0);  // 请求版本号，用于取消过期请求

  // 计算 source_id -> theme_color 映射
  const sourceColors = useMemo(() => {
    const map = new Map<number, string>();
    sources.forEach(s => {
      if (s.theme_color) {
        map.set(s.id, s.theme_color);
      }
    });
    return map;
  }, [sources]);

  // 构建二级导航项
  const navItems: SecondaryNavItem[] = useMemo(() => [
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
    { id: 'mine', icon: NavIcons.mine, label: t.brew.me, title: t.brew.me, ariaLabel: t.brew.me },
    { id: 'starred', icon: NavIcons.starred, label: t.brew.starred, title: t.brew.starred, ariaLabel: t.brew.starred },
  ], [t]);

  // 使用二级导航 Hook
  const { activeId, setActiveId, setExpanded } = useSecondaryNav({
    routePath: '/brew',
    items: navItems,
    defaultActiveId: 'all',
    expandHint: t.brew.expandMenu,
  });

  // 监听展开事件（来自导航岛的自动展开请求）
  useEffect(() => {
    const handleExpandSecondary = (e: CustomEvent<{ path: string }>) => {
      if (e.detail.path === '/brew') {
        setExpanded(true);
      }
    };

    window.addEventListener('nav-expand-secondary', handleExpandSecondary as EventListener);
    return () => {
      window.removeEventListener('nav-expand-secondary', handleExpandSecondary as EventListener);
    };
  }, [setExpanded]);

  // 监听导航变化
  // 用于追踪上一次的 activeId，避免 viewMode 变化导致重复执行
  const prevActiveIdRef = useRef(activeId);

  // 监听导航变化 - 只在 activeId 真正变化时执行
  useEffect(() => {
    // 如果 activeId 没有变化，不执行任何操作
    if (prevActiveIdRef.current === activeId) {
      return;
    }
    prevActiveIdRef.current = activeId;

    if (activeId === 'starred') {
      setViewMode('starred');
      setSelectedSource(null);
    } else if (activeId === 'friends') {
      setViewMode('sources');
      setSelectedCategory('friends');
      setSelectedSource(null);
    } else if (activeId === 'mine') {
      // "我"分类特殊处理：直接展示合并的文章列表，不显示网站卡片
      setViewMode('category-feed');
      setSelectedCategory('mine');
      setSelectedSource(null);
    } else if (activeId === 'all') {
      setViewMode('sources');
      setSelectedCategory('all');
      setSelectedSource(null);
    }
  }, [activeId, viewMode, selectedCategory, setActiveId]);

  // 加载订阅源列表
  const loadSources = useCallback(async () => {
    try {
      const data = await brewApi.getSources();
      setSources(data);
    } catch (err) {
      console.error('Failed to load sources:', err);
      setError('加载订阅源失败');
    }
  }, []);

  // 加载统计信息
  const loadStats = useCallback(async () => {
    try {
      const data = await brewApi.getStats();
      setStats(data);
    } catch (err) {
      console.error('Failed to load stats:', err);
    }
  }, []);

  // 加载文章列表 - 接受参数以避免闭包问题
  const loadItems = useCallback(async (reset = false, sourceId?: number, mode?: ViewMode, categoryFilter?: string) => {
    // 生成新的请求 ID，用于防止竞态条件
    const requestId = ++loadRequestIdRef.current;

    setItemsLoading(true);
    try {
      const currentPage = reset ? 1 : pageRef.current;
      const filter = mode === 'starred' ? 'starred' : 'all';

      const data = await brewApi.getItems({
        source_id: sourceId || undefined,
        category: categoryFilter || undefined,
        filter,
        page: currentPage,
        per_page: 20,
      });

      // 检查是否是最新的请求，忽略过期请求的响应
      if (requestId !== loadRequestIdRef.current) {
        return;
      }

      if (reset) {
        setItems(data.items);
        setPage(1);
        pageRef.current = 1;
      } else {
        setItems(prev => [...prev, ...data.items]);
      }

      setTotal(data.total);
      setHasMore(data.items.length >= 20);
    } catch (err) {
      // 忽略过期请求的错误
      if (requestId !== loadRequestIdRef.current) {
        return;
      }
      console.error('Failed to load items:', err);
      setError('加载文章失败');
    } finally {
      // 只有最新请求才更新 loading 状态
      if (requestId === loadRequestIdRef.current) {
        setItemsLoading(false);
      }
    }
  }, []);  // 移除 page 依赖，使用 ref

  // 初始加载
  useEffect(() => {
    const init = async () => {
      setLoading(true);
      await Promise.all([loadSources(), loadStats()]);
      setLoading(false);
    };
    init();
  }, [loadSources, loadStats]);

  // 当进入文章视图时加载文章
  useEffect(() => {
    if (viewMode === 'items' && selectedSource) {
      loadItems(true, selectedSource.id, viewMode);
    } else if (viewMode === 'starred') {
      loadItems(true, undefined, viewMode);
    } else if (viewMode === 'category-feed' && selectedCategory !== 'all') {
      // 分类合并文章视图：加载该分类下所有文章
      const categoryDbValue = PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId];
      loadItems(true, undefined, viewMode, categoryDbValue);
    }
  }, [viewMode, selectedSource, selectedCategory, loadItems]);

  // 处理源点击 - 进入该源的文章列表
  const handleSourceClick = useCallback((source: BrewSource) => {
    // 立即清空旧文章列表，避免切换时显示上一个源的内容
    setItems([]);
    setPage(1);
    pageRef.current = 1;
    setHasMore(true);
    setTotal(0);
    // 设置新源和视图模式
    setSelectedSource(source);
    setViewMode('items');
  }, []);

  // 处理订阅源更新（如卡片尺寸变更）
  const handleSourceUpdate = (updatedSource: BrewSource) => {
    setSources(prev =>
      prev.map(s => s.id === updatedSource.id ? updatedSource : s)
    );
  };

  // 处理返回源列表
  const handleBackToSources = useCallback(() => {
    setSelectedSource(null);
    setSelectedItem(null);
    setViewMode('sources');
    setActiveId(selectedCategory);
  }, [selectedCategory, setActiveId]);

  // 处理从分类合并文章视图返回
  const handleBackFromCategoryFeed = useCallback(() => {
    setSelectedItem(null);
    setViewMode('sources');
    setSelectedCategory('all');
    setActiveId('all');
  }, [setActiveId]);

  // 处理文章选择
  const handleItemSelect = async (item: BrewItem) => {
    // 如果未读，自动标记为已读
    if (!item.is_read) {
      // 先更新为已读状态再显示
      const updatedItem = { ...item, is_read: true };
      setSelectedItem(updatedItem);

      try {
        await brewApi.markRead(item.id);
        setItems(prev =>
          prev.map(i => i.id === item.id ? { ...i, is_read: true } : i)
        );
        setStats(prev => prev ? { ...prev, total_unread: prev.total_unread - 1 } : prev);
        // 同时更新 sources 的 unread_count 和 recent_items 中对应文章的 is_read
        setSources(prev =>
          prev.map(s => s.id === item.source_id ? {
            ...s,
            unread_count: s.unread_count - 1,
            recent_items: s.recent_items?.map(ri =>
              ri.id === item.id ? { ...ri, is_read: true } : ri
            )
          } : s)
        );
      } catch (err) {
        console.error('Failed to mark as read:', err);
        // API 失败时恢复状态
        setSelectedItem(item);
      }
    } else {
      setSelectedItem(item);
    }
  };

  // 处理收藏切换
  const handleToggleStar = async (item: BrewItem) => {
    try {
      if (item.is_starred) {
        await brewApi.unstarItem(item.id);
      } else {
        await brewApi.starItem(item.id);
      }

      setItems(prev =>
        prev.map(i => i.id === item.id ? { ...i, is_starred: !i.is_starred } : i)
      );

      if (selectedItem?.id === item.id) {
        setSelectedItem(prev => prev ? { ...prev, is_starred: !prev.is_starred } : prev);
      }

      setStats(prev => {
        if (!prev) return prev;
        return {
          ...prev,
          total_starred: item.is_starred ? prev.total_starred - 1 : prev.total_starred + 1,
        };
      });
    } catch (err) {
      console.error('Failed to toggle star:', err);
    }
  };

  // 处理添加订阅源
  const handleAddSource = async (url: string, name?: string, category?: string, icon?: string, sourceType?: 'link' | 'rss' | 'brewlia' | 'rsshub') => {
    const source = await brewApi.addSource({ url, name, category, source_type: sourceType });
    // 如果有自定义图标，添加后立即更新
    if (icon && source.id) {
      const updatedSource = await brewApi.updateSource(source.id, { icon });
      setSources(prev => [...prev, updatedSource]);
    } else {
      setSources(prev => [...prev, source]);
    }
    loadStats();
  };

  // 处理刷新订阅源
  const handleRefreshSource = async (sourceId: number) => {
    setSourceRefreshing(true);
    try {
      const newCount = await brewApi.refreshSource(sourceId);
      if (newCount > 0) {
        if (viewMode === 'items' && selectedSource?.id === sourceId) {
          loadItems(true, sourceId, viewMode);
        }
        loadStats();
        loadSources();
      }
    } catch (err) {
      console.error('Failed to refresh source:', err);
    } finally {
      setSourceRefreshing(false);
    }
  };

  // 处理全部标记已读
  const handleMarkAllRead = async () => {
    try {
      // 分类合并文章视图时，按分类标记已读
      const categoryFilter = viewMode === 'category-feed' && selectedCategory !== 'all'
        ? PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]
        : undefined;

      const marked = await brewApi.markAllRead({
        source_id: selectedSource?.id || undefined,
        category: categoryFilter,
      });

      if (marked > 0) {
        // 只更新当前列表中文章的已读状态，不重新加载列表，避免破坏排序
        setItems(prev => prev.map(item => ({ ...item, is_read: true })));
        // 更新订阅源的未读计数
        if (selectedSource) {
          setSources(prev => prev.map(s =>
            s.id === selectedSource.id ? { ...s, unread_count: 0 } : s
          ));
        } else if (categoryFilter) {
          // 分类视图：更新该分类下所有源的未读计数
          setSources(prev => prev.map(s => {
            if (!s.category) return s;
            const cats = s.category.split(',').map(c => c.trim());
            if (cats.includes(categoryFilter)) {
              return { ...s, unread_count: 0 };
            }
            return s;
          }));
        }
        loadStats();
      }
    } catch (err) {
      console.error('Failed to mark all read:', err);
    }
  };

  // 收藏页面批量管理
  const handleStarredEnterEditMode = useCallback(() => {
    setStarredEditMode(true);
    setStarredSelectedIds(new Set());
  }, []);

  const handleStarredExitEditMode = useCallback(() => {
    setStarredEditMode(false);
    setStarredSelectedIds(new Set());
  }, []);

  const handleStarredSelectAll = useCallback(() => {
    if (starredSelectedIds.size === items.length) {
      setStarredSelectedIds(new Set());
    } else {
      setStarredSelectedIds(new Set(items.map(i => i.id)));
    }
  }, [items, starredSelectedIds.size]);

  const handleStarredBatchUnstar = useCallback(async () => {
    if (starredSelectedIds.size === 0) return;

    setStarredProcessing(true);
    try {
      // 批量取消收藏
      const promises = Array.from(starredSelectedIds).map(id =>
        brewApi.unstarItem(id)
      );
      await Promise.all(promises);

      // 更新列表
      setItems(prev => prev.filter(i => !starredSelectedIds.has(i.id)));
      setTotal(prev => prev - starredSelectedIds.size);

      // 更新统计
      setStats(prev => prev ? {
        ...prev,
        total_starred: prev.total_starred - starredSelectedIds.size,
      } : prev);

      // 退出编辑模式
      handleStarredExitEditMode();
    } catch (err) {
      console.error('Failed to batch unstar:', err);
    } finally {
      setStarredProcessing(false);
    }
  }, [starredSelectedIds, handleStarredExitEditMode]);

  const handleStarredBack = useCallback(() => {
    setViewMode('sources');
    setActiveId('all');
    handleStarredExitEditMode();
  }, [setActiveId, handleStarredExitEditMode]);

  // 加载更多 - useCallback 缓存
  const handleLoadMore = useCallback(() => {
    if (!itemsLoading && hasMore) {
      pageRef.current += 1;
      setPage(pageRef.current);
      // 分类合并文章视图需要传递分类筛选
      const categoryFilter = viewMode === 'category-feed' && selectedCategory !== 'all'
        ? PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]
        : undefined;
      loadItems(false, selectedSource?.id, viewMode, categoryFilter);
    }
  }, [itemsLoading, hasMore, selectedSource?.id, viewMode, selectedCategory, loadItems]);

  // 关闭阅读器 - useCallback 缓存
  const handleCloseReader = useCallback(() => {
    setSelectedItem(null);
  }, []);

  // 处理已读/未读切换
  const handleToggleRead = async (item: BrewItem) => {
    try {
      if (item.is_read) {
        await brewApi.markUnread(item.id);
      } else {
        await brewApi.markRead(item.id);
      }

      const newReadState = !item.is_read;

      setItems(prev =>
        prev.map(i => i.id === item.id ? { ...i, is_read: newReadState } : i)
      );

      if (selectedItem?.id === item.id) {
        setSelectedItem(prev => prev ? { ...prev, is_read: newReadState } : prev);
      }

      setStats(prev => {
        if (!prev) return prev;
        return {
          ...prev,
          total_unread: newReadState ? prev.total_unread - 1 : prev.total_unread + 1,
        };
      });

      // 同时更新 sources 的 unread_count 和 recent_items 中对应文章的 is_read
      setSources(prev =>
        prev.map(s => s.id === item.source_id ? {
          ...s,
          unread_count: newReadState ? s.unread_count - 1 : s.unread_count + 1,
          recent_items: s.recent_items?.map(ri =>
            ri.id === item.id ? { ...ri, is_read: newReadState } : ri
          )
        } : s)
      );
    } catch (err) {
      console.error('Failed to toggle read:', err);
    }
  };

  // 键盘快捷键
  useBrewKeyboard({
    items,
    selectedItem,
    enabled: true,
    onSelectItem: (item) => item ? handleItemSelect(item) : handleCloseReader(),
    onToggleRead: handleToggleRead,
    onToggleStar: handleToggleStar,
    onRefresh: () => selectedSource && handleRefreshSource(selectedSource.id),
    onAddSource: () => {}, // 添加功能已整合到首页底部操作栏
    onMarkAllRead: handleMarkAllRead,
    onCloseReader: handleCloseReader,
    onShowHelp: () => {}, // 快捷键帮助已整合到首页底部操作栏
  });

  if (loading) {
    return (
      <AnimatedView className="min-h-screen flex items-center justify-center pt-20 pb-28 sm:pb-24 md:pb-12">
        <div className="flex flex-col items-center gap-4">
          <div className="w-8 h-8 border-2 border-orange-500 border-t-transparent rounded-full animate-spin" />
          <p className="text-gray-500">加载中...</p>
        </div>
      </AnimatedView>
    );
  }

  return (
    <AnimatedView className="min-h-screen">
      <div className="h-full flex flex-col pt-20 pb-28 sm:pb-24 md:pb-12 px-3 xs:px-4 sm:px-6">
        <div className="flex-1 max-w-7xl mx-auto w-full flex flex-col relative min-h-0">
          {/* 源列表视图 */}
        {viewMode === 'sources' && (
          <BrewSourceGrid
            sources={sources}
            category={selectedCategory === 'all' ? undefined : PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId]}
            onSourceClick={handleSourceClick}
            onRefreshSource={handleRefreshSource}
            onSourceUpdate={handleSourceUpdate}
            onSourcesChange={() => {
              loadSources();
              loadStats();
            }}
            onAddSource={handleAddSource}
            isAuthenticated={isAuthenticated}
            isAdmin={isAdmin}
          />
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
                setStarredSelectedIds(prev => {
                  const newSet = new Set(prev);
                  if (newSet.has(id)) {
                    newSet.delete(id);
                  } else {
                    newSet.add(id);
                  }
                  return newSet;
                });
              }}
            />
          </div>
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
                categoryName: PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId],
                categoryLabel: getCategoryName(selectedCategory as PresetCategoryId),
                total,
                unreadCount: sources
                  .filter(s => {
                    const targetCat = PRESET_CATEGORY_DB_VALUES[selectedCategory as PresetCategoryId];
                    if (!s.category) return false;
                    return s.category.split(',').map(c => c.trim()).includes(targetCat);
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

        {/* 阅读器 */}
        <AnimatePresence mode="wait">
          {selectedItem && (
            <BrewReader
              key={selectedItem.id}
              item={selectedItem}
              onClose={handleCloseReader}
              onToggleStar={() => handleToggleStar(selectedItem)}
              isAuthenticated={isAuthenticated}
              isAdmin={isAdmin}
              sourceType={sources.find(s => s.id === selectedItem.source_id)?.source_type}
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
  );
}
