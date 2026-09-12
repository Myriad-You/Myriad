/** 只做 Manifest 权限校验。运行时授权在 TappBridge 与后端 Runtime Grant。 */

import type { TappManifest } from '../types'
import { currentCopy, formatCurrent } from '../../i18n/localeCopy'
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
          formatCurrent(currentCopy().tapp.unknownPermission, {
            permission: String(permission),
          }),
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
