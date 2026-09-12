import type { TappCodeStructure, TappInstance } from '../types'
import type { LayerExecutionPlan } from './moduleRuntime'
import { buildLayerRuntime } from './moduleRuntime'

/** background 是 headless core，只跑共享层。 */
export type TappSandboxMode = 'widget' | 'page' | 'background'

/** 组装该模式入口。core 三种模式都先执行。 */
export function getLayerEntries(
  code: TappCodeStructure,
  mode: TappSandboxMode,
  widgetId?: string,
): string[] {
  const entries: string[] = []
  if (code.coreEntry) entries.push(code.coreEntry)
  if (mode === 'page' && code.pageEntry) entries.push(code.pageEntry)
  if (mode === 'widget' && widgetId) {
    // 只装这一个 widget 的入口。想共用就各自 require 同一文件。id 对不上必须失败。
    const widgetEntry = code.widgetEntries?.[widgetId]
    if (!widgetEntry) {
      throw new Error(`Unknown widget id: ${widgetId}`)
    }
    entries.push(widgetEntry)
  }
  return entries
}

export function buildLayerScript(
  code: TappCodeStructure,
  mode: TappSandboxMode,
  widgetId?: string,
): LayerExecutionPlan {
  return buildLayerRuntime(
    code.modules,
    getLayerEntries(code, mode, widgetId),
    code.moduleResolutions,
  )
}

function hashParts(parts: string[]): string {
  let fnv = 0x811C9DC5
  let djb = 5381
  let totalLength = 0
  for (const part of parts) {
    const framed = `${part.length}:`
    totalLength += part.length
    for (let index = 0; index < framed.length; index++) {
      const code = framed.charCodeAt(index)
      fnv ^= code
      fnv = Math.imul(fnv, 0x01000193)
      djb = Math.imul(djb, 33) ^ code
    }
    for (let index = 0; index < part.length; index++) {
      const code = part.charCodeAt(index)
      fnv ^= code
      fnv = Math.imul(fnv, 0x01000193)
      djb = Math.imul(djb, 33) ^ code
    }
  }
  return `${totalLength}:${(fnv >>> 0).toString(36)}:${(djb >>> 0).toString(36)}`
}

function sortedRecordParts(record?: Record<string, string>): string[] {
  if (!record) return []
  return Object.entries(record)
    .toSorted(([left], [right]) => left.localeCompare(right))
    .flatMap(([key, value]) => [key, value])
}

/** 只对该模式实际执行的模块取指纹。无关层变化不重建；依赖文件变了必须重建。 */
export function getCodeStructureFingerprint(
  code: TappCodeStructure,
  mode: TappSandboxMode,
  widgetId?: string,
): string {
  const entries = getLayerEntries(code, mode, widgetId)
  const { includedModules } = buildLayerRuntime(
    code.modules,
    entries,
    code.moduleResolutions,
  )
  const moduleParts = includedModules.flatMap((path) => [
    path,
    code.modules[path],
  ])

  if (mode === 'background') {
    return hashParts([...moduleParts, JSON.stringify(code.i18n || {})])
  }
  if (mode === 'widget') {
    return hashParts([
      ...moduleParts,
      code.widgetHtml || '',
      code.styles || '',
      ...sortedRecordParts(code.widgetStyles),
      code.widgetCSS || '',
      JSON.stringify(code.i18n || {}),
      ...sortedRecordParts(code.assets),
    ])
  }
  return hashParts([
    ...moduleParts,
    code.pageHtml || '',
    code.styles || '',
    code.pageStyles || '',
    code.pageCSS || '',
    JSON.stringify(code.i18n || {}),
    ...sortedRecordParts(code.assets),
  ])
}

export function getTappRuntimeFingerprint(instance: TappInstance): string {
  return hashParts([
    JSON.stringify(instance.manifest),
    JSON.stringify(instance.grantedPermissions.toSorted()),
    instance.userRole,
    String(instance.isTemporary ?? false),
    String(instance.isAdminTapp ?? false),
  ])
}
