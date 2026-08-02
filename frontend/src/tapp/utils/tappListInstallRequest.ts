/**
 * Pure resolution of `Tapp.tappList.install` request shape.
 *
 * Authority: must stay lockstep with the sandbox handler in
 * `runtime/sandbox/handlers/contentHandlers.ts` (store vs direct path).
 *
 * Catalog ref for store install comes only from:
 *   1. explicit `storeSource`, or
 *   2. `source` when it is an http(s) URL.
 * Bare non-HTTP `source` values (including numeric store ids like `"1"`) are
 * NOT treated as catalog refs unless also provided via `storeSource`.
 */

export interface TappListInstallRequestInput {
  source?: string
  tappId?: string
  storeSource?: string
  permissions?: string[]
  manifest?: unknown
  code?: string
  styles?: string
  pageTemplate?: string
  widgetTemplates?: Record<string, Record<string, string>>
  widgetCss?: string
  pageCss?: string
  i18n?: Record<string, unknown>
  pageModules?: Record<string, string>
  assets?: Record<string, string>
}

export type TappListInstallResolved =
  | {
      kind: 'direct'
      manifest: unknown
      code: string
      styles?: string
      pageTemplate?: string
      widgetTemplates?: Record<string, Record<string, string>>
      widgetCss?: string
      pageCss?: string
      i18n?: Record<string, unknown>
      pageModules?: Record<string, string>
      assets?: Record<string, string>
      permissions?: string[]
    }
  | {
      kind: 'store'
      /** Catalog URL or store source id — passed to installFromStore as `source`. */
      catalogRef: string
      tappId: string
      permissions?: string[]
    }
  | { kind: 'error'; error: string }

/**
 * Resolve SDK install args into direct/store/error without calling the network.
 * Used by the Bridge handler and by docs gating tests.
 */
export function resolveTappListInstallRequest(
  request: TappListInstallRequestInput | null | undefined,
): TappListInstallResolved {
  const rawSource = (request?.source || '').trim()
  const sourceLower = rawSource.toLowerCase()
  const isHttp =
    sourceLower.startsWith('https://') || sourceLower.startsWith('http://')
  const storeSourceCandidate = (
    request?.storeSource ||
    (isHttp ? rawSource : '') ||
    ''
  ).trim()

  if (sourceLower === 'direct') {
    if (!request?.manifest || typeof request.code !== 'string') {
      return {
        kind: 'error',
        error:
          'Direct install requires manifest and code (shared package missing)',
      }
    }
    return {
      kind: 'direct',
      manifest: request.manifest,
      code: request.code,
      styles: request.styles,
      pageTemplate: request.pageTemplate,
      widgetTemplates: request.widgetTemplates,
      widgetCss: request.widgetCss,
      pageCss: request.pageCss,
      i18n: request.i18n,
      pageModules: request.pageModules,
      assets: request.assets,
      permissions: request.permissions,
    }
  }

  // Store path: mode "store", or legacy where source itself is the catalog URL.
  const isStoreMode =
    sourceLower === 'store' ||
    isHttp ||
    (!!storeSourceCandidate && sourceLower !== 'direct')

  if (isStoreMode) {
    const tappId = request?.tappId
    if (!tappId) {
      return { kind: 'error', error: 'tappId is required for store install' }
    }
    // Prefer explicit storeSource; then HTTP source; never use mode "store".
    const catalogRef = storeSourceCandidate
    if (
      !catalogRef ||
      catalogRef.toLowerCase() === 'store' ||
      catalogRef.toLowerCase() === 'direct'
    ) {
      return {
        kind: 'error',
        error:
          'storeSource (catalog URL) is required for store install. Re-share the Tapp from Aro so the catalog URL is included.',
      }
    }
    return {
      kind: 'store',
      catalogRef,
      tappId,
      permissions: request?.permissions,
    }
  }

  return {
    kind: 'error',
    error: "Invalid source, must be 'direct' or 'store' (with storeSource)",
  }
}
