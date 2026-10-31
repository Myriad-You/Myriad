/** 只做 Manifest 权限校验。运行时授权在 TappBridge 与后端 Runtime Grant。 */

import type { TappManifest } from '../types'
import type { TappKernelIssue } from './kernelErrors'
import { PERMISSION_LEVELS } from './permissionConfig'

export class TappPermissionController {
  private constructor() {}

  static validateManifestPermissions(manifest: TappManifest): {
    valid: boolean
    errors: TappKernelIssue[]
    warnings: TappKernelIssue[]
  } {
    const errors: TappKernelIssue[] = []
    const warnings: TappKernelIssue[] = []

    for (const permission of manifest.permissions) {
      if (!PERMISSION_LEVELS[permission]) {
        errors.push({
          code: 'unknownPermission',
          params: { permission: String(permission) },
        })
      }
    }

    if (
      manifest.permissions.includes('platform:write') &&
      manifest.permissions.includes('ai:generate')
    ) {
      warnings.push({ code: 'sensitivePermissionCombo', params: {} })
    }

    return { valid: errors.length === 0, errors, warnings }
  }
}
