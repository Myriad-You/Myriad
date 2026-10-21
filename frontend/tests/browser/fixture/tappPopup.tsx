import type { WidgetConfig, WidgetPopupProps, WidgetType } from '../../../src/components/widgetGridTypes'
import type { TappInstance } from '../../../src/tapp/types'
import React, { useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter, useNavigate } from 'react-router-dom'
import { WidgetGridItemBody } from '../../../src/components/WidgetGridItemBody'
import WidgetPopupHost from '../../../src/components/widgetPopup/WidgetPopupHost'
import TappWidgetComponent from '../../../src/components/widgets/TappWidget'
import TappWidgetPopup from '../../../src/components/widgets/TappWidgetPopup'
import { I18nProvider } from '../../../src/contexts/I18nContext'
import { getWidgetPopupState } from '../../../src/lib/widgetPopup'
import { getResourceLoader } from '../../../src/tapp/runtime/sandbox/resourceLoader'
import { TappBridge } from '../../../src/tapp/runtime/TappBridge'
import { getTappRuntime, TappRuntime } from '../../../src/tapp/runtime/TappRuntime'
import { TappRuntimeGrant } from '../../../src/tapp/runtime/TappRuntimeGrant'

// Only installation discovery and package download are synthetic. The real card,
// popup host, both sandboxes, generated SDK, bridges and runtime grants run in Chromium.
const TAPP_ID = 'fixture.popup'
const WIDGET_ID = `${TAPP_ID}.card`
const instance = {
  id: TAPP_ID,
  manifest: {
    id: TAPP_ID, name: 'Popup', version: '1.0.0', category: 'utility', permissions: [],
    widgets: [{ id: 'card', name: 'Card', popup: { entry: 'popup.js', size: 'md' } }],
  },
  status: 'running', grantedPermissions: [], userRole: 'guest', installedAt: '',
} as unknown as TappInstance
TappRuntime.prototype.syncFromBackend = async () => {}
const runtime = getTappRuntime()
runtime.waitForSync = async () => {}
TappRuntime.prototype.getRegisteredWidgets = () => [{ id: WIDGET_ID, tappId: TAPP_ID, instanceCount: 0, registeredAt: '', config: { id: 'card', name: 'Card', defaultSize: '2x2', sizes: ['2x2'] } }]
TappRuntime.prototype.getTapp = () => instance
TappRuntime.prototype.isRunning = () => true
TappRuntime.prototype.canControlLifecycle = () => false
const loader = getResourceLoader()
loader.loadWidgetResources = async () => ({
  modules: { 'card.js': `
    Tapp.widget.openPopup({ from: 'boot' }).then(
      function() { document.body.setAttribute('data-boot-open', 'opened'); },
      function(error) { document.body.setAttribute('data-boot-open', String(error && error.message || error)); }
    );
    let opens = 0;
    Tapp.widgets.card = { render: function(root) {
      const button = document.createElement('button');
      button.textContent = 'Open popup';
      button.onclick = function() {
        opens++;
        Tapp.widget.openPopup({ n: opens }).then(
          function() { document.body.setAttribute('data-open', 'ok:' + opens); },
          function(error) { document.body.setAttribute('data-open', String(error && error.message || error)); }
        );
      };
      const arm = document.createElement('button');
      arm.textContent = 'Arm auto open';
      let autoOpens = 0;
      arm.onclick = function() {
        setInterval(function() {
          Tapp.widget.openPopup({ auto: true }).then(function() {
            autoOpens++;
            document.body.setAttribute('data-auto-opens', String(autoOpens));
          }, function() {});
        }, 300);
      };
      button.style.cssText = 'width:100%;height:70%';
      root.append(button, arm);
    } };
  ` }, widgetEntries: { card: 'card.js' }, html: '', css: '', size: '2x2',
})
loader.loadPopupResources = async () => ({
  modules: { 'popup.js': `
    const probe = { boot: crypto.randomUUID(), pauses: 0, resumes: 0 };
    const reflect = function() { for (const key of Object.keys(probe)) document.body.setAttribute('data-' + key, String(probe[key])); };
    reflect();
    Tapp.lifecycle.onPause(function() { probe.pauses++; reflect(); });
    Tapp.lifecycle.onResume(function() { probe.resumes++; reflect(); });
    Tapp.popups.card = { render: function(root, props) {
      const label = document.createElement('p');
      label.id = 'params';
      label.textContent = JSON.stringify(props.params);
      Tapp.popup.onParams(function(next) { label.textContent = JSON.stringify(next); });
      const close = document.createElement('button');
      close.textContent = 'Close from popup';
      close.onclick = function() { Tapp.popup.close().then(function() { document.body.setAttribute('data-close', 'ok'); }, function(error) { document.body.setAttribute('data-close', String(error && error.message || error)); }); };
      root.append(label, close);
    } };
  ` }, popupEntries: { card: 'popup.js' }, css: '',
})

const bridges: { surface: 'widget' | 'popup', bridge: TappBridge }[] = []
const initialize = TappBridge.prototype.initialize
TappBridge.prototype.initialize = function (...args: Parameters<typeof initialize>) {
  const iframe = args[0] as HTMLIFrameElement
  bridges.push({ surface: iframe.title.endsWith(' Popup') ? 'popup' : 'widget', bridge: this })
  return initialize.apply(this, args)
}

const widgetType: WidgetType = {
  id: WIDGET_ID,
  name: 'Card',
  defaultSize: '2x2',
  supportedSizes: ['2x2'],
  component: props => <TappWidgetComponent {...props} tappWidgetId={WIDGET_ID} />,
  isTappWidget: true,
  tappId: TAPP_ID,
  popup: {
    component: (props: WidgetPopupProps) => <TappWidgetPopup {...props} tappWidgetId={WIDGET_ID} />,
    size: 'md',
    chrome: 'close',
  },
} as WidgetType
const config: WidgetConfig = { id: 'card', type: WIDGET_ID, size: '2x2', position: { x: 0, y: 0 } } as WidgetConfig

function Fixture() {
  const anchorRef = useRef<HTMLDivElement>(null)
  const [mounted, setMounted] = useState(true)
  const navigate = useNavigate()
  return <>
    <div style={{ position: 'fixed', top: 0, right: 0, zIndex: 9999 }}>
      <button onClick={() => navigate('/elsewhere')}>Navigate</button>
      <button onClick={() => setMounted(false)}>Unmount card</button>
    </div>
    <div ref={anchorRef} style={{ width: 320, height: 220, marginTop: 60 }}>
      {mounted && <WidgetGridItemBody widget={config} widgetType={widgetType} isEditMode={false} anchorRef={anchorRef} />}
    </div>
    <WidgetPopupHost />
  </>
}

export function snapshot() {
  const state = getWidgetPopupState()
  const of = (surface: 'widget' | 'popup') => bridges.filter(entry => entry.surface === surface).map(entry => entry.bridge)
  const summary = (list: TappBridge[]) => ({
    bridges: list.length,
    destroyed: list.filter(bridge => bridge.isDestroyed()).length,
    active: list.filter(bridge => !bridge.isDestroyed() && bridge.isSurfaceActive()).length,
  })
  return {
    card: summary(of('widget')),
    popup: summary(of('popup')),
    entries: state.entries.length,
    activeKey: state.activeKey,
    grantRefs: TappRuntimeGrant.sharedWidgetRefCount(TAPP_ID),
  }
}

export function stopTapp() {
  ;(runtime as unknown as { emit: (event: string, data: unknown) => void }).emit('tapp:stopped', { id: TAPP_ID })
}

export function mount() {
  localStorage.setItem('locale', 'en-US')
  createRoot(document.getElementById('root')!).render(<I18nProvider><BrowserRouter><Fixture /></BrowserRouter></I18nProvider>)
}
