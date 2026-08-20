/**
 * 把包内 `.js` 文件编译成沙箱里可执行的一段经典脚本。
 *
 * 沙箱主动禁用了 `eval` 与 `Function`（runtime/sandbox/security.ts），所以模块
 * 工厂只能由宿主在生成 srcdoc 时静态拼成函数字面量注入：模块系统是宿主编译期
 * 的产物，不是沙箱内的运行时能力。不要试图在沙箱里 `new Function` 还原它。
 */

/** 层执行计划：要注入哪些模块，从哪些入口开始跑。 */
export interface LayerExecutionPlan {
  /** 注入 srcdoc 的脚本源码。 */
  source: string
  /** 实际打进这段脚本的模块路径，按依赖图顺序无关的稳定排序。 */
  includedModules: string[]
  /** 依次执行的入口，`core` 永远在最前。 */
  entries: string[]
}

const REQUIRE = 'require'
const IDENTIFIER_BYTE = /[\w$]/

/**
 * 顺序扫描出模块直接 `require` 的字面量目标（原样，未解析）。
 *
 * 跳过注释与字符串字面量，所以 `var s = "require('./x.js')"` 不是依赖；遇到真正的
 * `require(` 会立刻消费它的字符串参数，因此参数本身不会被当成「要跳过的字符串」。
 * 这与安装期的 `extract_require_requests` 同构，两边共用同一组用例
 * （见 moduleRuntime.test.ts 的 SHARED_EXTRACTION_SOURCE）。
 */
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

/**
 * 按 POSIX 规则解析相对路径。宿主侧解析一次，沙箱里只做表查找，
 * 这样「引用了不存在的文件」在生成阶段就能发现。
 */
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
      // 逃出包根不折叠回根内：折叠会让 `widget/index.js` 里的 `../../core.js`
      // 悄悄命中 `core.js`，作者拿不到写错了的信号。
      if (resolved.length === 0) return null
      resolved.pop()
      continue
    }
    resolved.push(segment)
  }
  return resolved.length > 0 ? resolved.join('/') : null
}

/** 解析候选：允许省略 `.js`，但不支持目录 index 与 json。 */
function resolveAgainstModules(
  fromModule: string,
  request: string,
  modules: Record<string, string>,
): string | undefined {
  const resolved = resolveModulePath(fromModule, request)
  if (resolved === null) return undefined
  if (resolved in modules) return resolved
  const withExtension = `${resolved}.js`
  if (withExtension in modules) return withExtension
  return undefined
}

/** 一个模块的 require 解析结果：原样请求 → 解析后的模块路径。 */
export type RequireResolution = Map<string, string>
/** 后端预解析的 require 表：模块路径 → 原样请求 → 目标模块路径。 */
export type ModuleResolutionTable = Record<string, Record<string, string>>

/** 静态解析一个模块直接 require 的目标。 */
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

/**
 * 从入口出发做闭包，只收该层真正用到的模块。
 *
 * 这条边界是有意的：widget 沙箱不该拿到 Page 的 JS，注入范围必须按依赖图算，
 * 不能因为「安装时扫描登记了整包」就把整包塞进每个 iframe。
 */
export function collectLayerModules(
  modules: Record<string, string>,
  entries: string[],
): {
  included: string[]
  missing: string[]
  /** 每个纳入模块的 require 解析表，供生成阶段直接复用。 */
  resolution: Map<string, RequireResolution>
} {
  const included: string[] = []
  const missing: string[] = []
  const resolution = new Map<string, RequireResolution>()
  const seen = new Set<string>()
  const queue = entries.filter((entry) => entry in modules)

  for (const entry of entries) {
    if (!(entry in modules)) missing.push(entry)
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

  return { included: included.sort(), missing, resolution }
}

function collectResolvedLayerModules(
  modules: Record<string, string>,
  entries: string[],
  resolutions: ModuleResolutionTable,
): { included: string[]; missing: string[] } {
  const included: string[] = []
  const missing: string[] = []
  const seen = new Set<string>()
  const queue = entries.filter((entry) => entry in modules)

  for (const entry of entries) {
    if (!(entry in modules)) missing.push(entry)
  }

  while (queue.length > 0) {
    const current = queue.shift()!
    if (seen.has(current)) continue
    seen.add(current)
    included.push(current)
    for (const [request, target] of Object.entries(
      resolutions[current] || {},
    )) {
      if (!(target in modules)) {
        missing.push(`${current} → ${request}`)
      } else if (!seen.has(target)) {
        queue.push(target)
      }
    }
  }

  return { included: included.sort(), missing }
}

function escapeModuleSource(source: string): string {
  // 模块体作为函数字面量嵌进外层 script，必须切断提前闭合 script 的可能。
  return source.replace(/<\/script/gi, '<\\/script')
}

/**
 * 生成一层的可执行脚本。
 *
 * 语义是 CommonJS 子集：同步 require、模块级缓存、循环依赖拿到部分导出、
 * 每个模块顶层声明彼此隔离。`Tapp` 与 `window._TAPP_*` 这些真全局不受影响。
 */
export function buildLayerRuntime(
  modules: Record<string, string>,
  entries: string[],
  moduleResolutions?: ModuleResolutionTable,
): LayerExecutionPlan {
  const orderedEntries = entries.filter((entry) => entry in modules)
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

  // 解析表沿用依赖图那一遍的结果：沙箱内只查表，路径解析不实现第二遍。
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
  var __factories = {
${registry}
  };
  var __resolve = ${JSON.stringify(resolutionTable)};
  var __cache = {};
  function __require(from, request) {
    var target = (__resolve[from] && __resolve[from][request]) || request;
    if (!Object.prototype.hasOwnProperty.call(__factories, target)) {
      throw new Error('Cannot find module ' + JSON.stringify(request) + ' from ' + JSON.stringify(from));
    }
    if (Object.prototype.hasOwnProperty.call(__cache, target)) {
      return __cache[target].exports;
    }
    var module = { exports: {} };
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
