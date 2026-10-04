import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { SocialNetworkSettingsModal, SocialNetworkWidget } from '../../../src/components/widgets/SocialNetworkWidget'
import { I18nProvider } from '../../../src/contexts/I18nContext'
import './socialNetworkIcons.css'

function Fixture() {
  const [generation, setGeneration] = useState(0)
  return <>
    <button onClick={() => setGeneration(value => value + 1)}>Remount widgets</button>
    <div key={generation} style={{ display: 'flex', gap: 24, padding: 40 }}>
      {['custom_qq', 'custom_telegram', 'custom_unknown', 'custom_url', 'github', 'bilibili'].map(platformId => (
        <div key={platformId} data-platform={platformId} style={{ width: 140, height: 140 }}>
          <SocialNetworkWidget
            config={{ id: platformId, type: 'social-network', size: '1x1', position: { x: 0, y: 0 }, config: { platformId } }}
            isEditMode
            isPreview
          />
        </div>
      ))}
    </div>
    <SocialNetworkSettingsModal />
  </>
}

createRoot(document.getElementById('root')!).render(<StrictMode><I18nProvider><Fixture /></I18nProvider></StrictMode>)
