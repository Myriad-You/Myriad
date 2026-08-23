/**
 * Manifest permission validation. Runtime authorization lives in TappBridge
 * and the backend Runtime Grant; this module intentionally keeps no usage or
 * role-derived authorization state.
 */

import type { TappManifest } from '../types'
import { currentCopy } from '../../i18n/localeCopy'
import { PERMISSION_LEVELS } from './permissionConfig'

export class TappPermissionController {
  private constructor() {}

  static validateManifestPermissions(manifest: TappManifest): {
    valid: boolean
    errors: string[]
    warnings: string[]
  } {
    const errors: string[] = []
    const warnings: string[] = []

    for (const permission of manifest.permissions) {
      if (!PERMISSION_LEVELS[permission]) {
        errors.push(
          currentCopy().tapp.unknownPermission.replace(
            '{permission}',
            String(permission),
          ),
        )
      }
    }

    if (
      manifest.permissions.includes('platform:write') &&
      manifest.permissions.includes('ai:generate')
    ) {
      warnings.push(currentCopy().tapp.sensitivePermissionCombo)
    }

    return { valid: errors.length === 0, errors, warnings }
  }
}
