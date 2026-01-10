/**
 * 页面级调度器 Hooks 统一导出
 *
 * 每个页面只导入自己需要的模块，实现真正的按需加载
 *
 * ## 使用方式
 *
 * ```tsx
 * // ❌ 不要这样（会加载所有页面的代码）
 * import { useHomeScheduler, useLibraryScheduler } from '@hooks/animation/pages';
 *
 * // ✅ 应该这样（只加载首页需要的代码）
 * import { useHomeScheduler, useHomeResize } from '@hooks/animation/pages/home';
 * ```
 *
 * ## 页面功能矩阵
 *
 * | 页面            | 功能                                           |
 * |-----------------|-----------------------------------------------|
 * | home            | Visibility, Resize, RAF, Idle                 |
 * | library         | Resize, Intersection, Idle                    |
 * | reports         | Visibility, Interval, RAF, DOMBatch           |
 * | config          | Timeout                                       |
 * | data-management | 无特殊需求                                     |
 * | login           | Timeout                                       |
 * | details         | 无特殊需求                                     |
 * | setup           | Timeout                                       |
 */

// Brew 页面
export {
  brewAnimationPresets,
  cleanupBrew,
  getBrewTransition,
  useBrewAnimationConfig,
  useBrewCardStagger,
  useBrewScheduler,
} from './brew'

// 首页
export {
  cleanupHome,
  useHomeIdle,
  useHomeRaf,
  useHomeResize,
  useHomeScheduler,
  useHomeVisibility,
} from './home'

// 资料库
export {
  cleanupLibrary,
  useLibraryInfiniteScroll,
  useLibraryInView,
  useLibraryLazyLoad,
  useLibraryPrefetch,
  useLibraryResize,
  useLibraryScheduler,
} from './library'

// 报告页
export {
  cleanupReports,
  useReportsBatchDom,
  useReportsInterval,
  useReportsRaf,
  useReportsRafThrottle,
  useReportsScheduler,
  useReportsTimeout,
  useReportsVisibility,
  useReportsVisibilityInterval,
} from './reports'

// 简单页面（Config、Login、Setup、DataManagement、Details）
export {
  useConfigScheduler,
  useDataManagementScheduler,
  useDetailsScheduler,
  useLoginScheduler,
  useSetupScheduler,
  useSimpleDebounce,
  useSimplePageScheduler,
  useSimpleThrottle,
  useSimpleTimeout,
} from './simple'

// Tapp 页面
export {
  cleanupTapp,
  useTappScheduler,
  useTappStagger,
  useTappVisibility,
} from './tapp'
