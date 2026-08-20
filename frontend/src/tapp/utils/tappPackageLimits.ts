import type { TappManifest } from '../types'

export const MAX_TAPP_ASSETS = 128
export const MAX_TAPP_GAME_ASSETS = 256
export const MAX_TAPP_GAME_ARCHIVE_BYTES = 128 * 1024 * 1024
/** Bridge budget for `tappList.install` / `getInstallPackage`. Game ZIP + slack. */
export const TAPP_PACKAGE_PAYLOAD_BYTES =
  MAX_TAPP_GAME_ARCHIVE_BYTES + 512 * 1024

/** Same predicate as `TappManifest::uses_game_package_limits`. */
export function usesGamePackageLimits(
  manifest: Pick<TappManifest, 'category' | 'game' | 'runtimeModules'>,
): boolean {
  if (manifest.category !== 'game' && manifest.category !== 'developer') {
    return false
  }
  const protocol = manifest.game?.protocol?.trim() ?? ''
  const hasRuntime =
    Array.isArray(manifest.runtimeModules) && manifest.runtimeModules.length > 0
  return Boolean(protocol || hasRuntime)
}

export function maxDeclaredAssets(
  manifest: Pick<TappManifest, 'category' | 'game' | 'runtimeModules'>,
): number {
  return usesGamePackageLimits(manifest) ? MAX_TAPP_GAME_ASSETS : MAX_TAPP_ASSETS
}
