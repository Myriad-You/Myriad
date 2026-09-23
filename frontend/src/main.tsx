import { createRoot } from 'react-dom/client'
import App from './App'
import { StartupFailure } from './components/DocumentReady'
import { RenderErrorBoundary } from './components/RenderErrorBoundary'
import { preloadLandingRoute } from './utils/codeSplitting'
import { initPageLoader } from './utils/pageLoader'
import { getUIConfigDeduped } from './utils/requestDedup'
import { initSiteMetadata } from './utils/siteMetadata'
import './styles/tailwind.css'

preloadLandingRoute(window.location.pathname)
// Every route's shell (wallpaper, dashboard, PWA) reads it; start it with the entry.
void getUIConfigDeduped().catch(() => {})
initPageLoader()
initSiteMetadata()
void import('./utils/pwa').then((m) => m.initPwaLifecycle())

createRoot(document.getElementById('app-root')!).render(
  <RenderErrorBoundary source="app" fallback={<StartupFailure />}>
    <App />
  </RenderErrorBoundary>,
)
