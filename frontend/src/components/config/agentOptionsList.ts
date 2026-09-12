export const AGENT_OPTIONS_LIST_SCROLL_AFTER = 8

export const AGENT_OPTIONS_LIST_PAGE = 30

export function agentOptionsListWindow(count: number): {
  maxHeight: '22rem' | null
  maxVisibleItems: number | null
} {
  if (count > AGENT_OPTIONS_LIST_SCROLL_AFTER) {
    return { maxHeight: '22rem', maxVisibleItems: AGENT_OPTIONS_LIST_PAGE }
  }
  return { maxHeight: null, maxVisibleItems: null }
}
