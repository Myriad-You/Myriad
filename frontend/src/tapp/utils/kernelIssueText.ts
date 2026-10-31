import type { TappKernelIssue } from '../runtime/kernelErrors'
import { currentCopy, formatCurrent } from '../../i18n/localeCopy'
import { formatKernelText, TAPP_KERNEL_ERROR_TEXT } from '../runtime/kernelErrors'

/** A kernel refusal in the site's language. */
export function kernelIssueText(issue: TappKernelIssue): string {
  const copy = currentCopy()
  switch (issue.code) {
    case 'unknownPermission':
      return formatCurrent(copy.tapp.unknownPermission, issue.params)
    case 'sensitivePermissionCombo':
      return copy.tapp.sensitivePermissionCombo
    case 'runtimeModuleHttp':
      return `${formatCurrent(copy.errors.httpStatus, {
        status: issue.params.status ?? 0,
      })} (${issue.params.id})`
    case 'runtimeModuleUnknown':
      // Only ever logged; the site has no copy for it.
      return formatKernelText(TAPP_KERNEL_ERROR_TEXT.runtimeModuleUnknown, issue.params)
  }
}
