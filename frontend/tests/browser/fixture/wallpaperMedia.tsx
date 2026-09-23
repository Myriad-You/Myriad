import React, { Suspense, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { WallpaperMediaPicker } from '../../../src/components/config/WallpaperMediaPicker'
import { I18nProvider } from '../../../src/contexts/I18nContext'

function Fixture() {
  const [url, setUrl] = useState('')
  return <><WallpaperMediaPicker onSelect={setUrl} /><output>{url}</output></>
}
createRoot(document.getElementById('root')!).render(<I18nProvider><Suspense fallback="Loading"><Fixture /></Suspense></I18nProvider>)
