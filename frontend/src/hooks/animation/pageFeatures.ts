/**
 * 页面级功能配置
 *
 * 定义每个页面需要的调度器功能，实现真正的按需加载
 * 未配置的功能不会初始化，实现零开销
 */

/** 功能标识 */
export enum Feature {
  /** 页面可见性监听 */
  Visibility = 1 << 0,
  /** ResizeObserver 尺寸监听 */
  Resize = 1 << 1,
  /** IntersectionObserver 视口监听 */
  Intersection = 1 << 2,
  /** 定时器（setInterval） */
  Interval = 1 << 3,
  /** 延时器（setTimeout） */
  Timeout = 1 << 4,
  /** requestAnimationFrame */
  RAF = 1 << 5,
  /** 空闲任务调度 */
  Idle = 1 << 6,
  /** DOM 批量读写 */
  DOMBatch = 1 << 7,
  /** MessageChannel 任务调度 */
  MessageChannel = 1 << 8,
}

/** 页面功能配置 */
export interface PageFeatureConfig {
  /** 页面 ID */
  pageId: string
  /** 启用的功能（位掩码） */
  features: number
  /** 功能描述（开发调试用） */
  description?: string
}

/**
 * 各页面功能配置
 *
 * 基于实际分析结果：
 *
 * | 页面            | 功能                                           |
 * |-----------------|-----------------------------------------------|
 * | home            | Visibility, Resize, RAF, Idle                 |
 * | library         | Resize, Intersection, Idle                    |
 * | reports         | Visibility, Interval, RAF, DOMBatch           |
 * | life            | Visibility                                    |
 * | config          | Timeout                                       |
 * | login           | Timeout                                       |
 * | details         | 基础（无特殊需求）                              |
 * | setup           | Timeout                                       |
 */
export const PAGE_FEATURES: Record<string, number> = {
  // 首页：Widget 拖拽(RAF)、响应式布局(Resize)、可见性感知、定时轮播(Interval)
  home:
    Feature.Visibility |
    Feature.Resize |
    Feature.RAF |
    Feature.Idle |
    Feature.Interval,

  // 资料库：无限滚动(Intersection)、响应式网格(Resize)、预加载(Idle)
  library: Feature.Resize | Feature.Intersection | Feature.Idle,

  // 报告页：轮播定时器(Interval)、背景动画(RAF)、DOM优化(DOMBatch)、可见性暂停
  reports:
    Feature.Visibility | Feature.Interval | Feature.RAF | Feature.DOMBatch,

  // 退役 /life 路由：只保留页面可见性，不再做精灵轮询。
  life: Feature.Visibility,

  // Brew 阅读页：文章列表无限滚动(Intersection)、可见性感知暂停轮询、卡片交错动画(Timeout)
  brew:
    Feature.Visibility | Feature.Intersection | Feature.Timeout | Feature.Idle,

  // 配置页：防抖保存(Timeout)
  config: Feature.Timeout,

  // 登录页：延迟跳转(Timeout)
  login: Feature.Timeout,

  // 详情页：占位页面
  details: 0,

  // 设置页：轮询检测(Timeout)
  setup: Feature.Timeout,

  // Tapp 多窗口模式：可见性感知暂停、空闲预加载
  'tapp-multi': Feature.Visibility | Feature.Idle,
}

/**
 * 检查页面是否需要某功能
 */
export function hasFeature(pageId: string, feature: Feature): boolean {
  const features = PAGE_FEATURES[pageId] ?? 0
  return (features & feature) !== 0
}

/**
 * 获取页面所有功能列表（调试用）
 */
export function getFeatureList(pageId: string): string[] {
  const features = PAGE_FEATURES[pageId] ?? 0
  const list: string[] = []

  if (features & Feature.Visibility) list.push('Visibility')
  if (features & Feature.Resize) list.push('Resize')
  if (features & Feature.Intersection) list.push('Intersection')
  if (features & Feature.Interval) list.push('Interval')
  if (features & Feature.Timeout) list.push('Timeout')
  if (features & Feature.RAF) list.push('RAF')
  if (features & Feature.Idle) list.push('Idle')
  if (features & Feature.DOMBatch) list.push('DOMBatch')
  if (features & Feature.MessageChannel) list.push('MessageChannel')

  return list
}
