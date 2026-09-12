/** 缺授予时可选命名空间仍以拒绝桩存在。session token 每实例一份，不进模板缓存键。 */

import type { TappInstance } from '../../types'
import type { WidgetSdkCaps } from './sdkBody'
import {
  generateSdkBody,
  resolveWidgetSdkCaps,
} from './sdkBody'
import { serializeSandboxScriptValue } from './security'

export type { WidgetSdkCaps }
export { resolveWidgetSdkCaps }

/** 同一 Tapp 只拼一次 SDK；每 iframe 只替换 session token，不得跨沙箱复用。 */
const WIDGET_SDK_TOKEN_PLACEHOLDER = '__TAPP_WIDGET_SESSION_TOKEN__'
let widgetSdkTemplateCache: { key: string; body: string } | null = null

function capsFingerprint(caps: WidgetSdkCaps): string {
  return [
    caps.ai,
    caps.platform,
    caps.analytics,
    caps.report,
    caps.media,
    caps.speech,
    caps.event,
    caps.agent,
    caps.scheduler,
  ]
    .map((v) => (v ? '1' : '0'))
    .join('')
}

export function generateWidgetSDK(
  tappInstance: TappInstance,
  sessionToken?: string,
): string {
  const { id, manifest, grantedPermissions } = tappInstance
  const token = sessionToken || ''
  const idLiteral = serializeSandboxScriptValue(id)
  const nameLiteral = serializeSandboxScriptValue(manifest.name)
  const versionLiteral = serializeSandboxScriptValue(manifest.version)
  const permissionsLiteral = serializeSandboxScriptValue(
    grantedPermissions || [],
  )
  const caps = resolveWidgetSdkCaps(grantedPermissions)
  const cacheKey = `${id}\0${manifest.name}\0${manifest.version}\0${permissionsLiteral}\0${capsFingerprint(caps)}`
  const placeholderLiteral = serializeSandboxScriptValue(
    WIDGET_SDK_TOKEN_PLACEHOLDER,
  )

  if (!widgetSdkTemplateCache || widgetSdkTemplateCache.key !== cacheKey) {
    widgetSdkTemplateCache = {
      key: cacheKey,
      body: generateSdkBody({
        surface: 'widget',
        idLiteral,
        nameLiteral,
        versionLiteral,
        tokenLiteral: placeholderLiteral,
        permissionsLiteral,
        gameTypeLiteral: '""',
        caps,
      }),
    }
  }

  return widgetSdkTemplateCache.body.replace(
    placeholderLiteral,
    serializeSandboxScriptValue(token),
  )
}
