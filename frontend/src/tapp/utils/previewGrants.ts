/**
 * Temporary Playground / preview grants (MYR-024).
 *
 * Manifest `permissions` are declarations for install-time approval. Preview
 * must never treat the full declaration list as `grantedPermissions` for real
 * host capabilities. Only this explicit allowlist may be exercised in-session;
 * everything else stays deny-by-default until install.
 *
 * Keep in sync with backend `PREVIEW_PERMISSIONS` in
 * `backend/src/api/tapp_playground/helpers.rs`.
 */

import type { TappPermission } from '../types'

/** Host capabilities available in temporary Playground preview only. */
export const PREVIEW_PERMISSIONS = [
  'storage:read',
  'storage:write',
  'ui:theme',
  'ui:confirm',
  'ui:fullscreen',
  /** Declared openUrls only; host still enforces allowlist. */
  'ui:openUrl',
] as const satisfies readonly TappPermission[]

const PREVIEW_PERMISSION_SET = new Set<string>(PREVIEW_PERMISSIONS)

export function isPreviewPermission(permission: string): boolean {
  return PREVIEW_PERMISSION_SET.has(permission)
}

/** Stable bridge/SDK error code for host APIs that preview does not execute. */
export const PREVIEW_UNAVAILABLE_CODE = 'PREVIEW_UNAVAILABLE'

export function previewUnavailableMessage(action: string): string {
  return `${action} is unavailable in temporary preview. Install the Tapp to use this capability.`
}

/**
 * Page auto-repair must not rewrite generated AI / federation code just because
 * the user clicked it inside Playground preview.
 */
export function isPlaygroundPreviewExpectedError(message: string): boolean {
  const raw = (message || '').trim()
  if (!raw) return false
  const lower = raw.toLowerCase()
  if (
    lower.includes('unavailable in temporary preview') ||
    lower.includes('disabled in temporary preview') ||
    lower.includes('preview_unavailable') ||
    lower.includes('unknown action:') ||
    lower.includes('package assets are unavailable in temporary preview') ||
    lower.includes('declared apis are disabled in temporary preview') ||
    lower.includes('notifications are disabled in temporary preview')
  ) {
    return true
  }
  const missing = raw.match(/Missing permission:\s*([\w:-]+)/i)
  if (missing && !isPreviewPermission(missing[1])) {
    return true
  }
  return false
}

/**
 * Intersect manifest declarations with temporary preview grants.
 * Undeclared allowlist entries are not auto-granted (deny-by-default).
 */
export function selectPreviewGrantedPermissions(
  declaredPermissions: readonly string[] | null | undefined,
): TappPermission[] {
  if (!declaredPermissions?.length) return []
  return declaredPermissions.filter((permission): permission is TappPermission =>
    isPreviewPermission(permission),
  )
}
