import type { PanelTab } from './panelTransition'

export type IslandClickAffordance = 'control' | 'notification'

/**
 * Collapsed 智能岛 click target.
 *
 * The island body is the control island. Notifications are a separate
 * affordance (badge / resident indicator), not the carousel slot or viewport.
 */
export function islandPanelTabForClick(input: {
  affordance: IslandClickAffordance
  carouselType?: string | null
  viewport?: string | null
}): PanelTab {
  void input.carouselType
  void input.viewport
  return input.affordance === 'notification' ? 'notifications' : 'control'
}
