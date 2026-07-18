/** Host-side Widget registry APIs. */

import type { RegisteredWidget, WidgetRegistration } from '../types'
import { apiRequest } from './TappHttpClient'

export async function getAllWidgets(): Promise<RegisteredWidget[]> {
  return apiRequest<RegisteredWidget[]>('/api/tapps/widgets')
}

export async function registerTappWidget(
  tappId: string,
  config: WidgetRegistration,
  runtimeGrant: string,
): Promise<RegisteredWidget> {
  const requestBody = {
    id: config.id,
    name: config.name,
    description: config.description,
    icon: config.icon,
    default_size: config.defaultSize,
    sizes: config.sizes,
    category: config.category,
    settings: config.settings || [],
    refresh_policy: config.refreshPolicy,
  }
  return apiRequest<RegisteredWidget>(
    `/api/tapps/${encodeURIComponent(tappId)}/widgets`,
    {
      method: 'POST',
      body: JSON.stringify(requestBody),
      runtimeGrant,
    },
  )
}

export async function unregisterTappWidget(
  tappId: string,
  widgetId: string,
  runtimeGrant: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/widgets/${encodeURIComponent(widgetId)}`,
    { method: 'DELETE', runtimeGrant },
  )
}
