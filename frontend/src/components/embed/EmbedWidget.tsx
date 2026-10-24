import type { WidgetConfig, WidgetSize } from '../widgetGridTypes'
import { Suspense, useEffect, useMemo } from 'react'
import { useParams, useSearchParams } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import useTappWidgets from '../../hooks/useTappWidgets'
import { noteBuiltinWidgets, preloadNoteBuiltinWidgets } from '../phantasi/notes/noteBuiltinWidgets'
import { decodeWidgetConfigAttr } from '../phantasi/notes/noteLayout'
import { RenderErrorBoundary } from '../RenderErrorBoundary'
import { widgetHostConfig } from '../widgetLibraryModel'
import { EmbedFrameReady } from './embedFrame'
import './embed.css'

/**
 * 博客正文里的一个小组件（crates/myriad-blog 把占位换成 `/embed/widget/{type}?size=&config=`）。
 * 框的大小由博客页按 data-size 定，这里铺满。
 *
 * 目录和笔记编辑器一样：内置小组件加 Tapp 小组件。Tapp 运行时按需加载、自己向后端同步，
 * 看的人是谁跟着这一页的登录态走，和主站首页一样。
 * 音乐播放器画不出来，按找不到处理：它控制的是主站的播放器，博客页上没有。
 */
const BLOG_UNSUPPORTED = new Set(['music-player'])

/**
 * 页面代码一到就按地址去拉小组件自己的代码，不等语言包、登录检查和这一页渲染完才发现要它。
 * Tapp 小组件（`tapp.{应用}.{小组件}`）先拉运行时和外壳。
 */
export function preloadEmbedWidget(pathname: string): void {
  const prefix = '/embed/widget/'
  if (!pathname.startsWith(prefix)) return
  let type: string
  try {
    type = decodeURIComponent(pathname.slice(prefix.length))
  } catch {
    return
  }
  if (type.startsWith('tapp.')) {
    void import('../../tapp/runtime/TappRuntime').catch(() => {})
    void import('../widgets/TappWidget').catch(() => {})
    return
  }
  preloadNoteBuiltinWidgets([type])
}

export function EmbedWidget() {
  const { type = '' } = useParams()
  const [params] = useSearchParams()
  const { t } = useI18n()
  const builtin = useMemo(
    () => noteBuiltinWidgets(t.widgets).find((entry) => entry.id === type && !BLOG_UNSUPPORTED.has(entry.id)),
    [t.widgets, type],
  )
  const tapp = useTappWidgets(!builtin)
  const widgetType = builtin ?? tapp.tappWidgets.find((entry) => entry.id === type)
  const size = params.get('size')
  const rawConfig = params.get('config')
  const config = useMemo<WidgetConfig | null>(() => {
    if (!widgetType) return null
    return {
      id: `embed-${widgetType.id}`,
      type: widgetType.id,
      size: (size as WidgetSize | null) || widgetType.defaultSize,
      position: { x: 0, y: 0 },
      config: { ...widgetHostConfig(widgetType.id), ...decodeWidgetConfigAttr(rawConfig) },
    }
  }, [rawConfig, size, widgetType])

  // 小组件里的普通链接在整页打开，不在这个小框里换页。
  useEffect(() => {
    const base = document.createElement('base')
    base.target = '_top'
    document.head.prepend(base)
    return () => base.remove()
  }, [])

  const missing = (
    <>
      <p className="embed-widget__missing">{t.embed.widgetUnavailable}</p>
      <EmbedFrameReady />
    </>
  )
  // Tapp 目录还没同步完时先空着，不闪「显示不了」。
  if (!widgetType && tapp.isLoading) return <main className="embed-root embed-widget" />
  if (!widgetType || !config) {
    return <main className="embed-root embed-widget">{missing}</main>
  }
  const WidgetComponent = widgetType.component
  return (
    <main className="embed-root embed-widget">
      <RenderErrorBoundary source="embed-widget" fallback={missing}>
        <Suspense fallback={null}>
          <WidgetComponent config={config} isEditMode={false} isPreview={false} />
          <EmbedFrameReady />
        </Suspense>
      </RenderErrorBoundary>
    </main>
  )
}
