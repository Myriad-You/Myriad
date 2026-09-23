import type { HomeLayoutMode } from '../utils/homeLayout'
import { API_URL } from '../config'
import { clearDedupCache } from '../utils/requestDedup'
import { apiService } from './api'

/** Fields `POST /api/config/dashboard` accepts; every write is a partial update. */
export type DashboardConfigPatch = Partial<{
  title: string
  layout: string
  layout_mode: HomeLayoutMode
  custom_platforms: string
  title_font: string
  title_font_size: number
  title_color: string
  widget_theme: string
}>

export interface SavedDashboardConfig {
  /** The stored layout, with published sticker URLs substituted. */
  layout?: unknown
}

/**
 * The one write path for the home dashboard configuration. CSRF, its refresh,
 * and session-failure handling belong to the shared client; callers only say
 * what changed. The cached public UI config is dropped so the next read sees it.
 */
export async function saveDashboardConfig(
  patch: DashboardConfigPatch,
  options: { signal?: AbortSignal } = {},
): Promise<SavedDashboardConfig> {
  const saved = await apiService.post<SavedDashboardConfig>('/config/dashboard', patch, options)
  clearDedupCache(`${API_URL}/api/config/ui`)
  return saved
}
