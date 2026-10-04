import type { TappInstance } from '../../../src/tapp/types'
import { createRoot } from 'react-dom/client'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { AuthProvider } from '../../../src/contexts/AuthContext'
import { I18nNamespace, I18nProvider } from '../../../src/contexts/I18nContext'
import { NavigationProvider } from '../../../src/contexts/NavigationContext'
import { executeFrontendAction } from '../../../src/services/agent'
import { TappWindowManager } from '../../../src/tapp/components/TappWindowManager'
import { TappRunPage } from '../../../src/tapp/pages/TappRunPage'
import { getResourceLoader } from '../../../src/tapp/runtime/sandbox/resourceLoader'
import { getTappRuntime, TappRuntime } from '../../../src/tapp/runtime/TappRuntime'
import './tappWindows.css'
import '../../../src/styles/theme.css'
import '../../../src/styles/utility.css'

const apps = ['One', 'Two', 'Three'].map((name) => ({
  id: `fixture.${name.toLowerCase()}`,
  manifest: {
    id: `fixture.${name.toLowerCase()}`,
    name: `Window ${name}`,
    version: '1.0.0',
    icon: 'app',
    category: 'utility',
    permissions: [],
    page: { entry: 'page.js' },
  },
  status: 'running',
  installationStatus: 'installed',
  grantedPermissions: [],
  userRole: 'guest',
  installedAt: '',
}) as unknown as TappInstance)

// Discovery/download are synthetic. The real pages, manager and sandbox run.
TappRuntime.prototype.syncFromBackend = async () => {}
const runtime = getTappRuntime()
runtime.waitForSync = async () => {}
runtime.getAllTapps = () => apps
runtime.getTapp = (id) => apps.find((app) => app.id === id)
runtime.isRunning = () => true
runtime.canControlLifecycle = () => false
getResourceLoader().loadPageResources = async () => ({
  modules: { 'page.js': `
    document.body.dataset.boot = crypto.randomUUID();
    const button = document.createElement('button');
    let count = 0;
    button.textContent = 'Count 0';
    button.addEventListener('click', function() { button.textContent = 'Count ' + (++count); });
    document.getElementById('tapp-content').appendChild(button);
  ` },
  pageEntry: 'page.js',
  css: '',
})

export function mount(mode: 'single' | 'multi') {
  localStorage.setItem('locale', 'en-US')
  localStorage.setItem('animation-preference', 'exlight')
  createRoot(document.getElementById('root')!).render(
    <I18nProvider>
      <I18nNamespace names={['tapp']}>
        <MemoryRouter initialEntries={['/tapp/run/fixture.one']}>
          <AuthProvider>
            <NavigationProvider>
              {mode === 'multi' ? (
                <TappWindowManager initialTappId="fixture.one" />
              ) : (
                <Routes><Route path="/tapp/run/:id" element={<TappRunPage />} /></Routes>
              )}
            </NavigationProvider>
          </AuthProvider>
        </MemoryRouter>
      </I18nNamespace>
    </I18nProvider>,
  )
}

export async function openWindow(id: string) {
  return executeFrontendAction({ type: 'open_window', tappId: id })
}
