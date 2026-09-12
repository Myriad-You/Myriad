/** 沙箱禁用 eval/Function；模块工厂由宿主注入。不要在沙箱里 new Function。 */

export interface LayerExecutionPlan {
  source: string
  includedModules: string[]
  entries: string[]
}

const REQUIRE = 'require'
const IDENTIFIER_BYTE = /[\w$]/

/** 跳过注释与字符串。与安装期 extract_require_requests 同构，共用 SHARED_EXTRACTION_SOURCE。 */
export function extractRequireRequests(source: string): string[] {
  const requests: string[] = []
  let index = 0
  let previousCodeChar = ''

  while (index < source.length) {
    const char = source[index]

    if (char === '/' && source[index + 1] === '/') {
      const end = source.indexOf('\n', index)
      index = end === -1 ? source.length : end
      continue
    }
    if (char === '/' && source[index + 1] === '*') {
      const end = source.indexOf('*/', index + 2)
      index = end === -1 ? source.length : end + 2
      continue
    }

    if (char === "'" || char === '"' || char === '`') {
      index += 1
      while (index < source.length) {
        if (source[index] === '\\') {
          index += 2
          continue
        }
        if (source[index] === char) {
          index += 1
          break
        }
        index += 1
      }
      previousCodeChar = char
      continue
    }

    if (char === 'r' && source.startsWith(REQUIRE, index)) {
      const atBoundary = !IDENTIFIER_BYTE.test(previousCodeChar)
      const rest = source.slice(index + REQUIRE.length).trimStart()
      if (atBoundary && rest.startsWith('(')) {
        const inner = rest.slice(1).trimStart()
        const quote = inner[0]
        if (quote === "'" || quote === '"') {
          const end = inner.indexOf(quote, 1)
          if (end > 1) requests.push(inner.slice(1, end))
        }
      }
      index += REQUIRE.length
      previousCodeChar = 'e'
      continue
    }

    if (!/\s/.test(char)) previousCodeChar = char
    index += 1
  }

  return requests
}

function dirnameOf(path: string): string {
  const index = path.lastIndexOf('/')
  return index === -1 ? '' : path.slice(0, index)
}

export function resolveModulePath(
  fromModule: string,
  request: string,
): string | null {
  const base = request.startsWith('/') ? '' : dirnameOf(fromModule)
  const segments = request.startsWith('/')
    ? request.slice(1).split('/')
    : `${base ? `${base}/` : ''}${request}`.split('/')

  const resolved: string[] = []
  for (const segment of segments) {
    if (segment === '' || segment === '.') continue
    if (segment === '..') {
      if (resolved.length === 0) return null
      resolved.pop()
      continue
    }
    resolved.push(segment)
  }
  return resolved.length > 0 ? resolved.join('/') : null
}

function resolveAgainstModules(
  fromModule: string,
  request: string,
  modules: Record<string, string>,
): string | undefined {
  const resolved = resolveModulePath(fromModule, request)
  if (resolved === null) return undefined
  if (Object.hasOwn(modules, resolved)) return resolved
  const withExtension = `${resolved}.js`
  if (Object.hasOwn(modules, withExtension)) return withExtension
  return undefined
}

export type RequireResolution = Map<string, string>
export type ModuleResolutionTable = Record<string, Record<string, string>>

export function collectRequires(
  fromModule: string,
  source: string,
  modules: Record<string, string>,
): { resolved: RequireResolution; missing: string[] } {
  const resolved: RequireResolution = new Map()
  const missing: string[] = []
  for (const request of extractRequireRequests(source)) {
    const target = resolveAgainstModules(fromModule, request, modules)
    if (target) resolved.set(request, target)
    else missing.push(request)
  }
  return { resolved, missing }
}

/** 从入口做闭包。widget 沙箱不拿 Page JS；注入按依赖图，不按整包。 */
export function collectLayerModules(
  modules: Record<string, string>,
  entries: string[],
): {
  included: string[]
  missing: string[]
  resolution: Map<string, RequireResolution>
} {
  const included: string[] = []
  const missing: string[] = []
  const resolution = new Map<string, RequireResolution>()
  const seen = new Set<string>()
  const queue = entries.filter((entry) => Object.hasOwn(modules, entry))

  for (const entry of entries) {
    if (!Object.hasOwn(modules, entry)) missing.push(entry)
  }

  while (queue.length > 0) {
    const current = queue.shift()!
    if (seen.has(current)) continue
    seen.add(current)
    included.push(current)
    const { resolved, missing: unresolved } = collectRequires(
      current,
      modules[current],
      modules,
    )
    if (resolved.size > 0) resolution.set(current, resolved)
    for (const target of resolved.values()) {
      if (!seen.has(target)) queue.push(target)
    }
    for (const request of unresolved) {
      missing.push(`${current} → ${request}`)
    }
  }

  return { included: included.toSorted(), missing, resolution }
}

function collectResolvedLayerModules(
  modules: Record<string, string>,
  entries: string[],
  resolutions: ModuleResolutionTable,
): { included: string[]; missing: string[] } {
  const included: string[] = []
  const missing: string[] = []
  const seen = new Set<string>()
  const queue = entries.filter((entry) => Object.hasOwn(modules, entry))

  for (const entry of entries) {
    if (!Object.hasOwn(modules, entry)) missing.push(entry)
  }

  while (queue.length > 0) {
    const current = queue.shift()!
    if (seen.has(current)) continue
    seen.add(current)
    included.push(current)
    for (const [request, target] of Object.entries(
      resolutions[current] || {},
    )) {
      if (!Object.hasOwn(modules, target)) {
        missing.push(`${current} → ${request}`)
      } else if (!seen.has(target)) {
        queue.push(target)
      }
    }
  }

  return { included: included.toSorted(), missing }
}

function escapeModuleSource(source: string): string {
  // 模块体嵌进 script，须切断提前闭合 script。
  return source.replaceAll(/<\/script/gi, '<\\/script')
}

export function buildLayerRuntime(
  modules: Record<string, string>,
  entries: string[],
  moduleResolutions?: ModuleResolutionTable,
): LayerExecutionPlan {
  const orderedEntries = entries.filter((entry) =>
    Object.hasOwn(modules, entry),
  )
  const collected = moduleResolutions
    ? collectResolvedLayerModules(modules, entries, moduleResolutions)
    : collectLayerModules(modules, entries)
  const included = collected.included

  if (included.length === 0) {
    return { source: '', includedModules: [], entries: [] }
  }

  const registry = included
    .map((path) => {
      const factory = `function (module, exports, require) {\n${escapeModuleSource(modules[path])}\n}`
      return `  ${JSON.stringify(path)}: ${factory}`
    })
    .join(',\n')

  const resolutionTable: Record<string, Record<string, string>> = {}
  if (moduleResolutions) {
    for (const path of included) {
      if (moduleResolutions[path]) {
        resolutionTable[path] = moduleResolutions[path]
      }
    }
  } else {
    const resolution = (collected as ReturnType<typeof collectLayerModules>)
      .resolution
    for (const [path, requests] of resolution) {
      resolutionTable[path] = Object.fromEntries(requests)
    }
  }

  return {
    source: `(function () {
  'use strict';
  const __factories = {
${registry}
  };
  const __resolve = ${JSON.stringify(resolutionTable)};
  const __cache = {};
  const __hasOwn = Object.hasOwn;
  function __require(from, request) {
    const target = (__resolve[from] && __resolve[from][request]) || request;
    if (!__hasOwn(__factories, target)) {
      throw new Error('Cannot find module ' + JSON.stringify(request) + ' from ' + JSON.stringify(from));
    }
    if (__hasOwn(__cache, target)) {
      return __cache[target].exports;
    }
    const module = { exports: {} };
    // 先入缓存再执行：循环依赖拿到的是部分导出，而不是无限递归。
    __cache[target] = module;
    __factories[target](module, module.exports, function (request) {
      return __require(target, request);
    });
    return module.exports;
  }
${orderedEntries.map((entry) => `  __require(null, ${JSON.stringify(entry)});`).join('\n')}
})();`,
    includedModules: included,
    entries: orderedEntries,
  }
}
