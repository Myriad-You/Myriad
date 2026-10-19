import { lazy } from 'react'
import { BrowserRouter, Route, Routes } from 'react-router-dom'
import { EmbedAdminOnly, useEmbedFrame } from './components/embed/embedFrame'
import { NamespacedPage } from './components/NamespacedPage'
import { AnimationPreferenceProvider } from './contexts/AnimationPreferenceContext'
import { AuthProvider } from './contexts/AuthContext'
import { I18nProvider } from './contexts/I18nContext'
import { LocaleAccountSync } from './contexts/LocaleAccountSync'
import './styles/fonts.css'
import './styles/theme.css'
import './styles/animations.css'
import './styles/page-transitions.css'
import './styles/utility.css'
import './styles/overrides.css'
import './styles/performance.css'

const loadOverlay = () => import('./views/StudioOverlayFrame')
const loadTool = () => import('./views/Studio')

// 一个页面只会是其中一种：一启动就去取，和登录检查同时进行，不等路由渲染到再开始。
if (window.location.pathname === '/embed/studio/overlay') void loadOverlay()
else void loadTool()

const StudioOverlayFrame = lazy(loadOverlay)
const Studio = lazy(loadTool)

function StudioRoutes() {
  useEmbedFrame()
  return (
    <Routes>
      <Route
        path="/embed/studio/overlay"
        element={
          <EmbedAdminOnly>
            <NamespacedPage names={['phantasi', 'studio']}>
              <StudioOverlayFrame />
            </NamespacedPage>
          </EmbedAdminOnly>
        }
      />
      <Route
        path="/embed/studio/tool/*"
        element={
          <EmbedAdminOnly>
            <NamespacedPage names={['phantasi', 'studio']}>
              <Studio />
            </NamespacedPage>
          </EmbedAdminOnly>
        }
      />
    </Routes>
  )
}

/** 创作台嵌入页的根：只有语言、动画偏好、登录态和两条路由。 */
export function StudioApp() {
  return (
    <BrowserRouter>
      <I18nProvider>
        <AnimationPreferenceProvider>
          <AuthProvider>
            <LocaleAccountSync />
            <StudioRoutes />
          </AuthProvider>
        </AnimationPreferenceProvider>
      </I18nProvider>
    </BrowserRouter>
  )
}
