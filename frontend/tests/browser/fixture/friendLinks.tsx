import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { MemoryRouter } from 'react-router-dom'
import { PhantasiFeaturedWidget } from '../../../src/components/phantasi/tiles/PhantasiFeaturedTile'
import { FriendLinksWidget } from '../../../src/components/widgets/FriendLinksWidget'
import { AnimationPreferenceProvider } from '../../../src/contexts/AnimationPreferenceContext'
import { AuthProvider } from '../../../src/contexts/AuthContext'
import { I18nProvider } from '../../../src/contexts/I18nContext'
import { NavigationProvider } from '../../../src/contexts/NavigationContext'
import { AppLayout } from '../../../src/layouts/AppLayout'
import '../../../src/styles/tailwind.css'

function Fixture() {
  const [mounted, setMounted] = useState(false)
  const [ordinary, setOrdinary] = useState(false)
  const preview = new URLSearchParams(location.search).has('preview')
  return (
    <>
      <button onClick={() => setMounted(true)}>Mount widget</button>
      <button onClick={() => setOrdinary(true)}>Mount ordinary sources</button>
      {ordinary && (
        <PhantasiFeaturedWidget
          config={{
            id: 'ordinary',
            type: 'phantasi',
            size: '4x2',
            position: { x: 0, y: 0 },
          }}
          isEditMode={false}
        />
      )}
      <div style={{ height: 2000 }} />
      <div id="widget" style={{ width: 400, height: 220 }}>
        {mounted && (
          <FriendLinksWidget
            config={{
              id: 'friends',
              type: 'friend-links',
              size: '4x2',
              position: { x: 0, y: 0 },
            }}
            isEditMode={false}
            isPreview={preview}
          />
        )}
      </div>
    </>
  )
}
createRoot(document.getElementById('root')!).render(
  <I18nProvider>
    <MemoryRouter>
      <AnimationPreferenceProvider>
        <AuthProvider>
          <NavigationProvider>
            {new URLSearchParams(location.search).has('layout') ? (
              <AppLayout>
                <Fixture />
              </AppLayout>
            ) : (
              <Fixture />
            )}
          </NavigationProvider>
        </AuthProvider>
      </AnimationPreferenceProvider>
    </MemoryRouter>
  </I18nProvider>,
)
