import { useState, useSyncExternalStore } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { usePageContentOptional } from '../../contexts/PageContentContext'
import { resolveAgentContext } from './agentContext'
import {
  getAgentContextConsent,
  getServerAgentContextConsent,
  setAgentContextConsent,
  subscribeAgentContextConsent,
} from './agentContextConsent'
import {
  getAgentSelectionSnapshot,
  getServerAgentSelectionSnapshot,
  selectionIsFresh,
  selectionPreview,
  subscribeAgentSelection,
} from './agentSelection'

export function useAgentPanelContext(pathname: string) {
  const { t } = useI18n()
  const pageContent = usePageContentOptional()
  const [openedAtMs] = useState(() => Date.now())

  const selectionSnapshot = useSyncExternalStore(
    subscribeAgentSelection,
    getAgentSelectionSnapshot,
    getServerAgentSelectionSnapshot,
  )
  const contextConsent = useSyncExternalStore(
    subscribeAgentContextConsent,
    getAgentContextConsent,
    getServerAgentContextConsent,
  )

  const selection = selectionIsFresh(selectionSnapshot, openedAtMs)
    ? selectionSnapshot.text
    : undefined

  const context = resolveAgentContext({
    pathname,
    pageTitle: pageContent?.pageContent?.title,
    hasPageContent: !!pageContent?.hasContent,
    selection,
    contextConsent,
  })

  const routeName = t.agentPanel.context.routes[context.route]
  const kicker =
    context.kind === 'selection'
      ? t.agentPanel.context.selected
      : context.kind === 'content'
        ? t.agentPanel.context.watching
        : t.agentPanel.context.onPage
  const text =
    context.kind === 'selection'
      ? selectionPreview(context.selection ?? '')
      : context.kind === 'content'
        ? (context.title ?? routeName)
        : contextConsent
          ? routeName
          : t.agentPanel.context.blind
  const label = `${kicker} ${text}`

  return {
    context,
    kicker,
    text,
    label,
    contextConsent,
    canMute: context.kind !== 'selection',
    setContextConsent: setAgentContextConsent,
  }
}
