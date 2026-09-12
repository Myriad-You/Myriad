/**
 * 与 contentHandlers 同步。目录引用只来自 storeSource，或 source 为 http(s) URL。裸非 HTTP
 * source（含数字 id）不是目录引用。
 */

export interface TappListInstallRequestInput {
  source?: string
  tappId?: string
  storeSource?: string
  permissions?: string[]
  manifest?: unknown
  modules?: Record<string, string>
  coreStyles?: string
  pageStyles?: string
  widgetStyles?: Record<string, string>
  pageTemplate?: string
  widgetTemplates?: Record<string, Record<string, string>>
  widgetCss?: string
  pageCss?: string
  i18n?: Record<string, unknown>
  assets?: Record<string, string>
}

export type TappListInstallResolved =
  | {
      kind: 'direct'
      manifest: unknown
      modules: Record<string, string>
      coreStyles?: string
      pageStyles?: string
      widgetStyles?: Record<string, string>
      pageTemplate?: string
      widgetTemplates?: Record<string, Record<string, string>>
      widgetCss?: string
      pageCss?: string
      i18n?: Record<string, unknown>
      assets?: Record<string, string>
      permissions?: string[]
    }
  | {
      kind: 'store'
      catalogRef: string
      tappId: string
      permissions?: string[]
    }
  | { kind: 'error'; error: string }

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
    if (
      !request?.manifest ||
      !request.modules ||
      Object.keys(request.modules).length === 0
    ) {
      return {
        kind: 'error',
        error:
          'Direct install requires manifest and modules (shared package missing)',
      }
    }
    return {
      kind: 'direct',
      manifest: request.manifest,
      modules: request.modules,
      coreStyles: request.coreStyles,
      pageStyles: request.pageStyles,
      widgetStyles: request.widgetStyles,
      pageTemplate: request.pageTemplate,
      widgetTemplates: request.widgetTemplates,
      widgetCss: request.widgetCss,
      pageCss: request.pageCss,
      i18n: request.i18n,
      assets: request.assets,
      permissions: request.permissions,
    }
  }

  const isStoreMode =
    sourceLower === 'store' ||
    isHttp ||
    (!!storeSourceCandidate && sourceLower !== 'direct')

  if (isStoreMode) {
    const tappId = request?.tappId
    if (!tappId) {
      return { kind: 'error', error: 'tappId is required for store install' }
    }
    // 优先显式 storeSource，其次 HTTP source；不用 mode store。
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
