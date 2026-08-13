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
  // Read-only theme access only; preview never grants the subscription
  // half — least privilege, subscription needs install-time approval.
  'ui:theme:read',
  'ui:confirm',
  'ui:fullscreen',
  /** Declared openUrls only; host still enforces allowlist. */
  'ui:openUrl',
] as const satisfies readonly TappPermission[]

const PREVIEW_PERMISSION_SET = new Set<string>(PREVIEW_PERMISSIONS)

export function isPreviewPermission(permission: string): boolean {
  return PREVIEW_PERMISSION_SET.has(permission)
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
