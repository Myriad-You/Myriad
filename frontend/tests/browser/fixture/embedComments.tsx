import { createRoot } from 'react-dom/client'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { EmbedComments } from '../../../src/components/embed/EmbedComments'
import { AuthProvider } from '../../../src/contexts/AuthContext'
import { I18nNamespace, I18nProvider } from '../../../src/contexts/I18nContext'
// 和 embedMain.tsx / EmbedApp.tsx 同一套全局样式：评论区要扛得住它们（全局的 textarea 聚焦描边之类）。
import '../../../src/styles/tailwind.css'
import '../../../src/styles/fonts.css'
import '../../../src/styles/theme.css'
import '../../../src/styles/animations.css'
import '../../../src/styles/page-transitions.css'
import '../../../src/styles/utility.css'
import '../../../src/styles/overrides.css'
import '../../../src/styles/performance.css'

document.documentElement.className = `embed-frame ${parent.document.documentElement.className || 'light'}`
createRoot(document.getElementById('app-root')!).render(
  <MemoryRouter initialEntries={['/embed/comments/7']}><I18nProvider><AuthProvider><I18nNamespace names={['embed']}>
    <Routes><Route path="/embed/comments/:itemId" element={<EmbedComments />} /></Routes>
  </I18nNamespace></AuthProvider></I18nProvider></MemoryRouter>,
)
