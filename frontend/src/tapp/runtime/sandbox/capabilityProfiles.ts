import type { TappBridge } from '../TappBridge'
import sandboxContract from '../../../../../shared/tapp_sandbox_contract.json' with { type: 'json' }

export type SandboxCapabilityProfile = 'page' | 'widget' | 'headless'

/**
 * 可见/控制面动作不得从后台 core 触及。数据在
 * `shared/tapp_sandbox_contract.json`（与 CLI/SDK 契约同源）。
 */
export const HEADLESS_DENIED_ACTIONS: readonly string[] =
  sandboxContract.capabilities.headlessDeniedActions

export function applySandboxCapabilityProfile(
  bridge: TappBridge,
  profile: SandboxCapabilityProfile,
): void {
  if (profile !== 'headless') return
  for (const action of HEADLESS_DENIED_ACTIONS) {
    bridge.unregisterHandler(action)
  }
}
