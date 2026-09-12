/** 序列化授予权限，不是声明或批准。 */

import type { TappInstance } from '../../types'
import type { SandboxCapabilityProfile } from './capabilityProfiles'
import { generateSdkBody } from './sdkBody'
import { serializeSandboxScriptValue } from './security'

export function generateFullSDK(
  tappInstance: TappInstance,
  sessionToken?: string,
  profile: Extract<SandboxCapabilityProfile, 'page' | 'headless'> = 'page',
): string {
  const { id, manifest, grantedPermissions } = tappInstance
  return generateSdkBody({
    surface: profile,
    idLiteral: serializeSandboxScriptValue(id),
    nameLiteral: serializeSandboxScriptValue(manifest.name),
    versionLiteral: serializeSandboxScriptValue(manifest.version),
    tokenLiteral: serializeSandboxScriptValue(sessionToken || ''),
    permissionsLiteral: serializeSandboxScriptValue(grantedPermissions),
    gameTypeLiteral: serializeSandboxScriptValue(
      `game:${id}:${(manifest.game?.protocol || 'session').trim() || 'session'}`,
    ),
  })
}
