import type { TappCodeStructure, TappInstance } from '../types'
import type { LayerExecutionPlan } from './moduleRuntime'
import { buildLayerRuntime } from './moduleRuntime'

/** 沙箱模式。`background` 是 headless core，只跑共享层。 */
export type TappSandboxMode = 'widget' | 'page' | 'background'

/**
 * 组装某个模式要执行的入口序列。
 *
 * core 是三层共享层，三种模式都先执行它——模块化只改变层内文件从哪来，
 * 不改变 core 的地位。后台专属逻辑靠 `_TAPP_MODE === 'core'` 自行守卫。
 */
export function getLayerEntries(
  code: TappCodeStructure,
  mode: TappSandboxMode,
  widgetId?: string,
): string[] {
  const entries: string[] = []
  if (code.coreEntry) entries.push(code.coreEntry)
  if (mode === 'page' && code.pageEntry) entries.push(code.pageEntry)
  if (mode === 'widget' && widgetId) {
    // 只装这一个 widget 的入口。同一 Tapp 的其它 widget 不进这个 iframe——
    // 想共用代码就各自 require 同一个文件。id 对不上必须失败，不能退化成
    // 「只跑 core」——那看起来像成功的空 widget。
    const widgetEntry = code.widgetEntries?.[widgetId]
    if (!widgetEntry) {
      throw new Error(`Unknown widget id: ${widgetId}`)
    }
    entries.push(widgetEntry)
  }
  return entries
}

/**
 * 编译某个模式要注入 iframe 的脚本。
 *
 * 返回执行计划而不是裸字符串：调用方常常还需要知道实际装入了哪些模块（调试输出、
 * 指纹），分开取会导致同一份依赖图被算两遍。
 */
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
    .sort(([left], [right]) => left.localeCompare(right))
    .flatMap(([key, value]) => [key, value])
}

/**
 * 内容级 iframe 指纹。
 *
 * 只对该模式实际执行的模块取指纹：无关层的代码变化不该重建这个 iframe，
 * 而它真正依赖的文件变了必须重建（等长更新也要能识别出来）。
 */
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
    ])
  }
  return hashParts([
    ...moduleParts,
    code.pageHtml || '',
    code.styles || '',
    code.pageStyles || '',
    code.pageCSS || '',
    JSON.stringify(code.i18n || {}),
  ])
}

/** Manifest/runtime contract changes require a new SDK and handler set. */
export function getTappRuntimeFingerprint(instance: TappInstance): string {
  return hashParts([
    JSON.stringify(instance.manifest),
    JSON.stringify([...instance.grantedPermissions].sort()),
    instance.userRole,
    String(instance.isTemporary ?? false),
    String(instance.isAdminTapp ?? false),
  ])
}
