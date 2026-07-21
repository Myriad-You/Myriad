/**
 * Site public domain change API (non-federation).
 *
 * POST /api/admin/site/domain — rewrites BASE_URL, FRONTEND_URL, CORS_ORIGINS.
 * Does not perform ActivityPub / federation Move.
 */

import apiService from './api'

export interface DomainChecklistItem {
  key: string
  status: 'auto' | 'manual' | string
  summary: string
}

export interface DomainMigrationChecklist {
  dns: DomainChecklistItem
  tls: DomainChecklistItem
  reverse_proxy_301: DomainChecklistItem
  oauth_callbacks: DomainChecklistItem
  federation_move_separate: DomainChecklistItem
  backend_restart_for_cors: DomainChecklistItem
}

export interface ChangeSiteDomainApplied {
  base_url: string
  frontend_url: string
  cors_origins: string
  previous_origin: string | null
}

export interface ChangeSiteDomainResponse {
  success: boolean
  message: string
  applied?: ChangeSiteDomainApplied
  checklist?: DomainMigrationChecklist
}

export interface ChangeSiteDomainRequest {
  new_origin: string
  previous_origin?: string
}

export async function changeSiteDomain(
  body: ChangeSiteDomainRequest,
): Promise<ChangeSiteDomainResponse> {
  return apiService.post<ChangeSiteDomainResponse>('/admin/site/domain', body)
}

/** Flatten checklist object into ordered items for UI. */
export function checklistItems(
  checklist: DomainMigrationChecklist | undefined,
): DomainChecklistItem[] {
  if (!checklist) return []
  return [
    checklist.dns,
    checklist.tls,
    checklist.reverse_proxy_301,
    checklist.oauth_callbacks,
    checklist.backend_restart_for_cors,
    checklist.federation_move_separate,
  ].filter(Boolean)
}
