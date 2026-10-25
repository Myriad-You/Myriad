import { createRoot } from 'react-dom/client'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { EmbedComments } from '../../../src/components/embed/EmbedComments'
import { AuthProvider } from '../../../src/contexts/AuthContext'
import { I18nNamespace, I18nProvider } from '../../../src/contexts/I18nContext'
import '../../../src/styles/theme.css'

document.documentElement.className = `embed-frame ${parent.document.documentElement.className || 'light'}`
createRoot(document.getElementById('app-root')!).render(
  <MemoryRouter initialEntries={['/embed/comments/7']}><I18nProvider><AuthProvider><I18nNamespace names={['embed']}>
    <Routes><Route path="/embed/comments/:itemId" element={<EmbedComments />} /></Routes>
  </I18nNamespace></AuthProvider></I18nProvider></MemoryRouter>,
)
