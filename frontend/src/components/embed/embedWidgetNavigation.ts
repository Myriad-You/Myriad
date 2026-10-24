import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router-dom'
import { embedHost } from './embedHost'

/**
 * 小组件用路由跳转（比如快捷方式打开某个应用）时，换外面那一页，不在框里换。
 * 只管从小组件页起步的框；挂在嵌入页的路由外面（跳走之后小组件页已经不渲染了），
 * 单独一个文件，不把小组件的代码带进嵌入页的入口。
 */
export function EmbedWidgetNavigation() {
  const location = useLocation()
  const start = useRef(location.key)
  const fromWidget = useRef(location.pathname.startsWith('/embed/widget/'))
  useEffect(() => {
    if (!fromWidget.current || location.key === start.current) return
    const href = `${location.pathname}${location.search}${location.hash}`
    ;(embedHost()?.win ?? window).location.assign(href)
  }, [location])
  return null
}
