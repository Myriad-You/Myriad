import { lazyWithPreload } from '../../utils/codeSplitting'

/**
 * 宿主的唯一入口：AppLayout 渲染它，卡片上预加载它。预加载过的模块同一轮就渲染，
 * 不先提交 Suspense 占位——占位换成内容要被 React 推迟最多 300ms，点开会空等。
 */
export const WidgetPopupHost = lazyWithPreload(() => import('./WidgetPopupHost'))
export const loadWidgetPopupHost = WidgetPopupHost.preload
