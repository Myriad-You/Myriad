import { lazy, Suspense, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useAuth } from '../../../contexts/AuthContext'
import { I18nNamespace } from '../../../contexts/I18nContext'
import { NavigationProvider } from '../../../contexts/NavigationContext'
import { PageContentProvider } from '../../../contexts/PageContentContext'
import { canUseAgent, useModuleVisibilityPreferences } from '../../../utils/moduleVisibility'
import { usePermissionConfig } from '../../../utils/permissionConfig'
import { dispatchAgentPanelCommand } from '../../agent-panel/agentPanelEvents'
import { setAgentPanelMode } from '../../agent-panel/agentPanelMode'

const AgentEngine = lazy(() => import('../../agent-panel/AgentEngine'))
const AgentPanel = lazy(() => import('../../agent-panel/AgentPanel'))

/**
 * Every opened note starts its own Work conversation: nothing from the main site or from the
 * last note carries over. Rendered after the engine so its command listener is already there.
 */
function FreshWorkConversation() {
  useEffect(() => {
    setAgentPanelMode('work')
    dispatchAgentPanelCommand('new-session')
  }, [])
  return null
}

/**
 * The site's global Agent with its composer always present at the foot of the editor. Each
 * note gets a fresh Work conversation, with no session list to wander into; the page it sees
 * is the open note (see noteAgentPage). Rendered into the editor's dock from outside the editor's React subtree,
 * so typing here is never mistaken for editing the article.
 */
export function NoteResidentAgent({ dock }: { dock: HTMLElement | null }) {
  const { isAuthenticated, isAdmin, hasChecked } = useAuth()
  const { preferences, isLoading } = useModuleVisibilityPreferences()
  const { elevatedAiChat, loaded } = usePermissionConfig()
  const allowed = hasChecked && !isLoading && loaded
    && canUseAgent(preferences, { isAuthenticated, isAdmin }, elevatedAiChat)

  if (!allowed || !dock) return null
  return createPortal(
    <NavigationProvider>
      <PageContentProvider>
        <I18nNamespace names={['merope', 'agentCaps']}>
          <Suspense fallback={null}>
            <AgentEngine />
            <AgentPanel resident />
            <FreshWorkConversation />
          </Suspense>
        </I18nNamespace>
      </PageContentProvider>
    </NavigationProvider>,
    dock,
  )
}
