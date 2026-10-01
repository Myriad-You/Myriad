/** 预览不得把声明权限当作授予。仅此 allowlist 可在会话内行使；其余 deny-by-default。与 backend PREVIEW_PERMISSIONS 同步。 */

import type { TappPermission } from '../types'

export const PREVIEW_PERMISSIONS = [
  'storage:read',
  'storage:write',
  'ui:theme',
  'ui:confirm',
  'ui:fullscreen',
  /** 仅声明的 openUrls；宿主仍强制 allowlist。 */
  'ui:openUrl',
  /** 预览无 Runtime Grant，读不到站内数据；放声明的 remoteMedia 让作者看到图。 */
  'media:remote',
] as const satisfies readonly TappPermission[]

const PREVIEW_PERMISSION_SET = new Set<string>(PREVIEW_PERMISSIONS)

export function isPreviewPermission(permission: string): boolean {
  return PREVIEW_PERMISSION_SET.has(permission)
}

export const PREVIEW_UNAVAILABLE_CODE = 'PREVIEW_UNAVAILABLE'

export function previewUnavailableMessage(action: string): string {
  return `${action} is unavailable in temporary preview. Install the Tapp to use this capability.`
}

/** 预览里点 AI/联邦失败不得触发 page auto-repair。 */
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

/** 预览没有安装批准这一步：声明了 media:remote 就放声明的域名（CSP 侧仍会再过滤字符集）。 */
export function selectPreviewGrantedRemoteMedia(
  declaredPermissions: readonly string[] | null | undefined,
  declaredRemoteMedia: readonly string[] | null | undefined,
): string[] {
  if (!declaredPermissions?.includes('media:remote')) return []
  return [...(declaredRemoteMedia ?? [])]
}

/** 声明 ∩ 预览 allowlist。未声明的 allowlist 项不自动授予。 */
export function selectPreviewGrantedPermissions(
  declaredPermissions: readonly string[] | null | undefined,
): TappPermission[] {
  if (!declaredPermissions?.length) return []
  return declaredPermissions.filter((permission): permission is TappPermission =>
    isPreviewPermission(permission),
  )
}
