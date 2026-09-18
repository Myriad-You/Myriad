import type { ComponentType } from 'react'
import type { WidgetComponentProps } from '../widgetGridTypes'

// 渲染期不能 React.lazy/Suspense 拆 face（多卡入场会坏）。
import {

  createElement,
  useEffect,
  useReducer,
} from 'react'
import { Spinner } from '../Spinner'

type ReportCardModule = typeof import('./ReportCardWidget')

let shell: ReportCardModule | null = null
let shellInflight: Promise<ReportCardModule> | null = null

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

export function preloadReportCardsForTypes(
  types: Iterable<string>,
): Promise<void> {
  const list = Iterator.from(types).toArray()
  const hasReport = list.some((t) => t.startsWith('report-'))
  if (!hasReport) return Promise.resolve()
  return Promise.all([
    ensureReportCardShell(),
    import('./reportCard/platformFaceLoaders').then((m) =>
      m.preloadPlatformFacesForWidgetTypes(list),
    ),
  ]).then(() => undefined)
}

function ReportCardHost(props: WidgetComponentProps) {
  const [, rerender] = useReducer((n: number) => n + 1, 0)

  useEffect(() => {
    if (shell) return
    let alive = true
    // 漏预热至少拉壳；face 由 PlatformFace 按 platformId 补拉。
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

// 目录 preload 只拉壳；完整预热走 preloadReportCardsForTypes。
;(ReportCardHost as unknown as { preload: () => Promise<void> }).preload =
  ensureReportCardShell

export { ReportCardHost }
