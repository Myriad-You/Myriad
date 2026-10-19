/**
 * 创作台嵌入页的入口（`studio.html`）：`/embed/studio/overlay` 和 `/embed/studio/tool/*`。
 * 它们只在博客页的覆盖层里，每开一个窗口就启动一次，所以不带主站外壳（导航、助手、
 * 音乐、Tapp、PWA、站点配置），只带这两类页面用得上的东西。
 */

import { createRoot } from 'react-dom/client'
import { StartupFailure } from './components/DocumentReady'
import { RenderErrorBoundary } from './components/RenderErrorBoundary'
import { announceStudioWindow } from './components/studio/studioIsland'
import { StudioApp } from './StudioApp'
import { initPageLoader } from './utils/pageLoader'
import './styles/tailwind.css'

initPageLoader()
announceStudioWindow()

createRoot(document.getElementById('app-root')!).render(
  <RenderErrorBoundary source="app" fallback={<StartupFailure />}>
    <StudioApp />
  </RenderErrorBoundary>,
)
