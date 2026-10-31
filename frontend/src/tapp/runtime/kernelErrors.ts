/**
 * Why the Tapp kernel refused, in English. The kernel is the part of the
 * runtime every host shares (bridge, sandbox, grant); it reads no locale
 * catalog. Each host shows its own words for each code, filling them with the
 * params.
 */
export const TAPP_KERNEL_ERROR_TEXT = {
  unknownPermission: 'Unknown permission: {permission}',
  sensitivePermissionCombo:
    'This app asks for both data write and AI generation. Only continue if you trust the source.',
  runtimeModuleUnknown: 'Unknown runtime module: {id}',
  runtimeModuleHttp: 'HTTP {status} while loading runtime module {id}',
} as const

export type TappKernelErrorCode = keyof typeof TAPP_KERNEL_ERROR_TEXT
export type TappKernelErrorParams = Record<string, string | number>

/** A refusal as data: what went wrong, with nothing worded yet. */
export interface TappKernelIssue {
  code: TappKernelErrorCode
  params: TappKernelErrorParams
}

/** `{name}` interpolation with no locale catalog. */
export function formatKernelText(
  template: string,
  params: TappKernelErrorParams = {},
): string {
  return template.replaceAll(/\{(\w+)\}/g, (_, key: string) => {
    const value = params[key]
    return value == null ? `{${key}}` : String(value)
  })
}

export class TappKernelError extends Error {
  override readonly name = 'TappKernelError'

  constructor(
    readonly code: TappKernelErrorCode,
    readonly params: TappKernelErrorParams = {},
  ) {
    super(formatKernelText(TAPP_KERNEL_ERROR_TEXT[code], params))
  }
}
