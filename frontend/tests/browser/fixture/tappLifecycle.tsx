import type { TappInstance } from '@myriad/tapp-runtime'
import { TappBridge } from '@myriad/tapp-runtime'
import React, { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import TappWidgetComponent from '../../../src/components/widgets/TappWidget'
import { AuthProvider } from '../../../src/contexts/AuthContext'
import { I18nProvider } from '../../../src/contexts/I18nContext'
import { getTappRuntime, TappRuntime } from '../../../src/tapp/runtime/TappRuntime'
import { tappKernel } from '../../../src/tapp/tappKernel'

// Only installation discovery and package download are synthetic. The real widget,
// observer, sandbox, generated SDK, bridge and runtime grant execute in Chromium.
const instance = {
  id: 'fixture.lifecycle', manifest: { id: 'fixture.lifecycle', name: 'Lifecycle', version: '1.0.0', category: 'utility', permissions: [] },
  status: 'running', grantedPermissions: [], userRole: 'guest', installedAt: '',
} as unknown as TappInstance
TappRuntime.prototype.syncFromBackend = async () => {}
const runtime = getTappRuntime()
runtime.waitForSync = async () => {}
TappRuntime.prototype.getRegisteredWidgets = () => [{ id: 'fixture.lifecycle.card', tappId: instance.id, instanceCount: 0, registeredAt: '', config: { id: 'card', name: 'Card', defaultSize: '2x2', sizes: ['2x2'] } }]
TappRuntime.prototype.getTapp = () => instance
TappRuntime.prototype.isRunning = () => true
TappRuntime.prototype.canControlLifecycle = () => false
tappKernel.resources.loadWidgetResources = async () => ({
  modules: { 'card.js': `
    const probe = { boot: crypto.randomUUID(), pauses: 0, resumes: 0 };
    const reflect = function() { for (const key of Object.keys(probe)) document.body.setAttribute('data-' + key, String(probe[key])); };
    reflect();
    Tapp.lifecycle.onPause(function() { probe.pauses++; reflect(); });
    Tapp.lifecycle.onResume(function() { probe.resumes++; reflect(); });
    Tapp.widgets.card = { render: function(root) { root.textContent = 'Lifecycle ready'; } };
  ` }, widgetEntries: { card: 'card.js' }, html: '', css: '', size: '2x2',
})
const bridges: TappBridge[] = []
const initialize = TappBridge.prototype.initialize
TappBridge.prototype.initialize = function (...args: Parameters<typeof initialize>) {
  bridges.push(this)
  return initialize.apply(this, args)
}
const config = { id: 'card', type: 'fixture.lifecycle.card', size: '2x2' as const, position: { x: 0, y: 0 } }

function Fixture() {
  const [visible, setVisible] = useState(true)
  const [mounted, setMounted] = useState(true)
  return <>
    <div style={{ position: 'fixed', top: 0, right: 0, zIndex: 999 }}>
      <button onClick={() => setVisible(value => !value)}>Toggle viewport</button>
      <button onClick={() => setMounted(false)}>Unmount</button>
    </div>
    <div style={{ width: 320, height: 220, marginTop: visible ? 40 : 3000 }}>
      {mounted && <TappWidgetComponent tappWidgetId="fixture.lifecycle.card" config={config} isEditMode={false} />}
    </div>
  </>
}

export function snapshot() {
  return {
    bridges: bridges.length,
    destroyed: bridges.filter(bridge => bridge.isDestroyed()).length,
    active: bridges.filter(bridge => bridge.isSurfaceActive()).length,
    grantRefs: tappKernel.grants.sharedWidgetRefCount(instance.id),
  }
}

export function mount() {
  localStorage.setItem('locale', 'en-US')
  createRoot(document.getElementById('root')!).render(<I18nProvider><BrowserRouter><Fixture /></BrowserRouter></I18nProvider>)
}

export function mountWithAuth() {
  localStorage.setItem('locale', 'en-US')
  createRoot(document.getElementById('root')!).render(<I18nProvider><BrowserRouter><AuthProvider><Fixture /></AuthProvider></BrowserRouter></I18nProvider>)
}
