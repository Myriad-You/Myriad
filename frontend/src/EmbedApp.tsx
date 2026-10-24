import { lazy } from 'react'
import { BrowserRouter, Route, Routes } from 'react-router-dom'
import { DocumentReady } from './components/DocumentReady'
import { EmbedAdminOnly } from './components/embed/embedFrame'
import { EmbedWidgetNavigation } from './components/embed/embedWidgetNavigation'
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

const loaders = {
  overlay: () => import('./views/StudioOverlayFrame'),
  tool: () => import('./views/Studio'),
  comments: () => import('./views/EmbedComments'),
  friend: () => import('./views/EmbedFriendApply'),
  companion: () => import('./views/EmbedCompanion'),
  widget: () => import('./views/EmbedWidget'),
}

/** 地址对应哪一页；对不上的是 null，渲染空白——嵌入页不把主站画进博客页的框里。 */
function pageOf(pathname: string): keyof typeof loaders | null {
  if (pathname === '/embed/studio/overlay') return 'overlay'
  if (pathname.startsWith('/embed/studio/tool/')) return 'tool'
  if (pathname.startsWith('/embed/comments/')) return 'comments'
  if (pathname === '/embed/friend-apply') return 'friend'
  if (pathname.startsWith('/embed/companion/')) return 'companion'
  if (pathname.startsWith('/embed/widget/')) return 'widget'
  return null
}

// 一个页面只会是其中一种：一启动就去取，和登录检查同时进行，不等路由渲染到再开始。
const page = pageOf(window.location.pathname)
if (page === 'widget') {
  void loaders.widget().then((module) => module.preloadEmbedWidget(window.location.pathname))
} else if (page) {
  void loaders[page]()
}

const StudioOverlayFrame = lazy(loaders.overlay)
const Studio = lazy(loaders.tool)
const EmbedComments = lazy(loaders.comments)
const EmbedFriend = lazy(loaders.friend)
const EmbedCompanion = lazy(loaders.companion)
const EmbedWidget = lazy(loaders.widget)

function EmbedRoutes() {
  return (
    <Routes>
      <Route
        path="/embed/comments/:itemId"
        element={
          <NamespacedPage names={['embed']}>
            <EmbedComments />
          </NamespacedPage>
        }
      />
      <Route
        path="/embed/friend-apply"
        element={
          <NamespacedPage names={['embed']}>
            <EmbedFriend />
          </NamespacedPage>
        }
      />
      <Route
        path="/embed/companion/:itemId"
        element={
          <NamespacedPage names={['embed']}>
            <EmbedCompanion />
          </NamespacedPage>
        }
      />
      <Route
        path="/embed/widget/:type"
        element={
          <NamespacedPage names={['embed']}>
            <EmbedWidget />
          </NamespacedPage>
        }
      />
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
      <Route path="*" element={<DocumentReady>{null}</DocumentReady>} />
    </Routes>
  )
}

/** 嵌入页的根：只有语言、动画偏好、登录态和这几条路由。 */
export function EmbedApp() {
  return (
    <BrowserRouter>
      <I18nProvider>
        <AnimationPreferenceProvider>
          <AuthProvider>
            <LocaleAccountSync />
            <EmbedWidgetNavigation />
            <EmbedRoutes />
          </AuthProvider>
        </AnimationPreferenceProvider>
      </I18nProvider>
    </BrowserRouter>
  )
}
