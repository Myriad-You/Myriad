/**
 * 嵌入页的入口（`embed.html`）：`/embed/*`——伴读、评论、友链申请、创作台的控制器和工具窗口。
 * 它们只在博客页里，访客每打开一篇文章就启动伴读和评论，创作台每开一个窗口启动一次，
 * 所以不带主站外壳（导航、助手、音乐、Tapp、PWA、站点配置），只带这些页面用得上的东西。
 */

import { createRoot } from 'react-dom/client'
import { StartupFailure } from './components/DocumentReady'
import { RenderErrorBoundary } from './components/RenderErrorBoundary'
import { announceStudioWindow } from './components/studio/studioIsland'
import { EmbedApp } from './EmbedApp'
import { initPageLoader } from './utils/pageLoader'
import './features/merope/character/installFigureHost'
import './styles/tailwind.css'

initPageLoader()
announceStudioWindow()

createRoot(document.getElementById('app-root')!).render(
  <RenderErrorBoundary source="app" fallback={<StartupFailure />}>
    <EmbedApp />
  </RenderErrorBoundary>,
)
