/**
 * PERMISSION_LEVELS 锁到后端 TappPermission 目录；动作表见
 * `shared/tapp_sandbox_contract.json`。speech / phantasiList / federation 先改 fixtures。
 */

import type { PermissionLevel, TappPermission } from '../types'
import sandboxContract from '../../../../shared/tapp_sandbox_contract.json' with { type: 'json' }

type TappPermissionLevel = Exclude<PermissionLevel, 'public'>

export const PERMISSION_LEVELS: Record<TappPermission, TappPermissionLevel> = {
  'widget:register': 'privileged',
  'platform:read': 'basic',
  'platform:write': 'privileged',
  'platform:register': 'privileged',
  'analytics:read': 'basic',
  'ai:generate': 'elevated',
  'ai:analyze': 'elevated',
  'ai:chat': 'elevated',
  'ai:image': 'elevated',
  'ai:search': 'elevated',
  '3d:generate': 'elevated',
  'report:read': 'basic',
  'report:write': 'privileged',
  'storage:read': 'basic',
  'storage:write': 'elevated',
  'ui:notification': 'basic',
  'ui:fullscreen': 'basic',
  'ui:theme': 'basic',
  'ui:confirm': 'basic',
  'ui:openUrl': 'basic',
  'network:fetch': 'elevated',
  'media:control': 'basic',
  'media:read': 'basic',
  'media:audio': 'basic',
  'component:theme': 'elevated',
  'component:agent': 'privileged',
  'shortcut:register': 'elevated',
  'event:publish': 'elevated',
  'event:subscribe': 'basic',
  'scheduler:register': 'elevated',
  'speech:tts': 'elevated',
  'speech:asr': 'elevated',
  'tappList:read': 'basic',
  'tappList:manage': 'privileged',
  'phantasi:read': 'basic',
  'phantasi:write': 'basic',
  'phantasi:commentWrite': 'elevated',
  'phantasi:manage': 'privileged',
  'federation:read': 'basic',
  'federation:post': 'elevated',
  'federation:interact': 'basic',
  'federation:channel': 'elevated',
  'federation:room': 'elevated',
  'federation:ring': 'basic',
  'federation:message': 'basic',
  'federation:trust': 'privileged',
  'federation:files': 'basic',
  'game:session': 'basic',
}

/**
 * Sandbox bridge action → required permission. The table lives in
 * `shared/tapp_sandbox_contract.json`, the single authority also exported by
 * `crates/tapp-contract` for the Tapp CLI/SDK; edit it there.
 */
export const PERMISSION_MAP: ReadonlyMap<string, TappPermission | 'public'> =
  new Map(
    Object.entries(sandboxContract.actions) as [string, TappPermission | 'public'][],
  )
