import { Suspense, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { UpdaterInlinePanel } from '../../../src/components/config/UpdaterConfigSection'
import { I18nProvider } from '../../../src/contexts/I18nContext'

function Fixture() {
  const [shown, setShown] = useState(true)
  return <I18nProvider><button onClick={() => setShown(value => !value)}>Toggle updater</button><Suspense fallback={<p>Loading</p>}>{shown && <UpdaterInlinePanel />}</Suspense></I18nProvider>
}
createRoot(document.getElementById('root')!).render(<Fixture />)
