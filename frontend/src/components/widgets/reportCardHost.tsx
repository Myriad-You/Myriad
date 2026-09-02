import type { ComponentType } from 'react'
import type { WidgetComponentProps } from '../WidgetGrid'
/**
 * 首页目录用的报告卡宿主。
 *
 * 约束：渲染期不能 React.lazy / Suspense 拆 face（多卡入场会坏）。
 *
 * 加载策略：
 * 1) 无 report-*：本文件极轻，不拉报告代码
 * 2) 有 report-*：挂载前
 *    - ensureReportCardShell() → 壳（ReportCardWidget + PlatformFace 调度）
 *    - preloadPlatformFaces(布局里用到的平台) → 仅相关 face chunk
 *    再 setWidgets，同步渲染，不挂起
 */
import {

  createElement,
  useEffect,
  useReducer,
} from 'react'
import { Spinner } from '../Spinner'
import { preloadPlatformFacesForWidgetTypes } from './reportCard/platformFaceLoaders'

type ReportCardModule = typeof import('./ReportCardWidget')

let shell: ReportCardModule | null = null
let shellInflight: Promise<ReportCardModule> | null = null

/** 只拉报告卡壳（不含各平台 face 实现） */
export function ensureReportCardShell(): Promise<void> {
  if (shell) return Promise.resolve()
  shellInflight ||= import('./ReportCardWidget').then((m) => {
    shell = m
    return m
  })
  return shellInflight.then(() => undefined).catch((err) => {
    shellInflight = null
    throw err
  })
}

/**
 * 按布局 types 预热：壳 + 用到的平台 face。
 * Home applyWidgets / 编辑模式预热应走这里。
 */
export function preloadReportCardsForTypes(
  types: Iterable<string>,
): Promise<void> {
  const list = Array.from(types)
  const hasReport = list.some((t) => t.startsWith('report-'))
  if (!hasReport) return Promise.resolve()
  return Promise.all([
    ensureReportCardShell(),
    preloadPlatformFacesForWidgetTypes(list),
  ]).then(() => undefined)
}

function ReportCardHost(props: WidgetComponentProps) {
  const [, rerender] = useReducer((n: number) => n + 1, 0)

  useEffect(() => {
    if (shell) return
    let alive = true
    // 漏预热兜底：至少拉壳；face 由 PlatformFace 按 platformId 补拉
    void ensureReportCardShell()
      .then(() => {
        if (alive) rerender()
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  const Impl = shell?.ReportCardWidget as
    | ComponentType<WidgetComponentProps>
    | undefined
  if (!Impl) {
    return (
      <div className="flex h-full w-full items-center justify-center">
        <Spinner size="lg" color="primary" />
      </div>
    )
  }
  return createElement(Impl, props)
}

ReportCardHost.displayName = 'ReportCardHost'

// 目录 preload(component) 时：只拉壳。
// 完整「壳+用到的 face」由 preloadBuiltinWidgets → preloadReportCardsForTypes 负责。
;(ReportCardHost as unknown as { preload: () => Promise<void> }).preload =
  ensureReportCardShell

export { ReportCardHost }
