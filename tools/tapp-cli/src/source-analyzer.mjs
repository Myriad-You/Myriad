import { readdir, readFile } from 'node:fs/promises'
import { extname, join, relative, sep } from 'node:path'
import ts from 'typescript'

const contract = JSON.parse(
  await readFile(new URL('./generated/contract.json', import.meta.url), 'utf8'),
)
const ACTIONS = contract.permissions.actions
const HEADLESS_DENIED_ACTIONS = new Set(contract.capabilities?.headlessDeniedActions || [])
const CODE_EXTENSIONS = new Set(contract.rules.sourceCodeExtensions)
const SKIP_DIRECTORIES = new Set(contract.rules.sourceScanSkipDirectories)
const ASSET_LITERAL_METHODS = new Set(contract.rules.assetLiteralMethods)
const ASSET_DIRECTORY = contract.rules.assetDirectory
const API_PUBLIC_ACCESS = contract.schema.$defs.TappApiAccess.enum.find(
  (value) => value === 'public',
)

function diagnostic(severity, code, message, file, line, column) {
  return { severity, code, message, file, line, column }
}

function addRequiredPermission(required, permission, reason, file) {
  if (!permission || permission === API_PUBLIC_ACCESS) return
  const entry = required.get(permission) || { permission, reasons: [], locations: [] }
  if (!entry.reasons.includes(reason)) entry.reasons.push(reason)
  if (!entry.locations.includes(file)) entry.locations.push(file)
  required.set(permission, entry)
}

function scriptKind(file) {
  if (file.endsWith('.tsx')) return ts.ScriptKind.TSX
  if (file.endsWith('.jsx')) return ts.ScriptKind.JSX
  if (file.endsWith('.ts')) return ts.ScriptKind.TS
  return ts.ScriptKind.JS
}

function unwrapExpression(node) {
  while (
    ts.isParenthesizedExpression(node) ||
    ts.isAsExpression(node) ||
    ts.isTypeAssertionExpression(node) ||
    ts.isNonNullExpression(node) ||
    ts.isSatisfiesExpression(node)
  ) {
    node = node.expression
  }
  return node
}

function staticMemberName(node) {
  if (ts.isPropertyAccessExpression(node)) return node.name.text
  if (ts.isElementAccessExpression(node)) {
    const argument = unwrapExpression(node.argumentExpression)
    if (ts.isStringLiteralLike(argument)) return argument.text
  }
  return undefined
}

function tappCallPath(expression) {
  const path = []
  let current = unwrapExpression(expression)
  while (ts.isPropertyAccessExpression(current) || ts.isElementAccessExpression(current)) {
    const name = staticMemberName(current)
    if (name === undefined) return undefined
    path.unshift(name)
    current = unwrapExpression(current.expression)
  }
  return ts.isIdentifier(current) && current.text === 'Tapp' ? path : undefined
}

function resolveAction(path) {
  for (let end = path.length; end >= 2; end -= 1) {
    const action = path.slice(0, end).join('.')
    if (Object.hasOwn(ACTIONS, action)) return action
  }
  return undefined
}

function literalString(node) {
  const value = node && unwrapExpression(node)
  return value && ts.isStringLiteralLike(value) ? value.text : undefined
}

const REMOTE_ENGINE_SCRIPT =
  /(?:src\s*=\s*['"]https?:\/\/[^'"]*(?:cdn\.jsdelivr\.net|unpkg\.com|cdnjs\.cloudflare\.com|esm\.sh|skypack\.dev|threejs\.org)|(?:import|from)\s*\(?\s*['"]https?:\/\/)/i

function warnRemoteEngineScript(source, file, diagnostics) {
  if (!REMOTE_ENGINE_SCRIPT.test(source)) return
  diagnostics.push(
    diagnostic(
      'warning',
      'remote-engine-script',
      'Sandbox cannot load engine scripts from a CDN. Bundle Three.js or other WebGL libraries into page/ as an IIFE and require it from the page entry.',
      file,
    ),
  )
}

function inspectCode(source, file, manifest, diagnostics, requiredPermissions, usedActions, surfaces) {
  warnRemoteEngineScript(source, file, diagnostics)
  const sourceFile = ts.createSourceFile(
    file,
    source,
    ts.ScriptTarget.Latest,
    true,
    scriptKind(file),
  )

  for (const parseDiagnostic of sourceFile.parseDiagnostics) {
    const location = sourceFile.getLineAndCharacterOfPosition(parseDiagnostic.start || 0)
    diagnostics.push(
      diagnostic(
        'error',
        'invalid-source-syntax',
        ts.flattenDiagnosticMessageText(parseDiagnostic.messageText, '\n'),
        file,
        location.line + 1,
        location.character + 1,
      ),
    )
  }

  function visit(node) {
    if (!ts.isCallExpression(node)) {
      ts.forEachChild(node, visit)
      return
    }

    const path = tappCallPath(node.expression)
    if (!path) {
      ts.forEachChild(node, visit)
      return
    }
    const position = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile))
    const location = { line: position.line + 1, column: position.character + 1 }

    const action = resolveAction(path)
    if (action) {
      const permission = ACTIONS[action]
      usedActions.push({ action, permission, file, ...location })
      addRequiredPermission(requiredPermissions, permission, `code calls ${action}`, file)
      if (HEADLESS_DENIED_ACTIONS.has(action)) {
        if (surfaces?.headlessOnly) {
          diagnostics.push(diagnostic('error', 'headless-denied-action', `${action} is unavailable in headless-only Tapps`, file, location.line, location.column))
        } else if (surfaces?.headless) {
          diagnostics.push(diagnostic('warning', 'headless-unavailable-action', `${action} is unavailable in headless core; keep it in Page/Widget code paths`, file, location.line, location.column))
        }
      }
    }

    if (path.length === 1 && path[0] === 'api') {
      const name = literalString(node.arguments[0])
      if (name === undefined) {
        diagnostics.push(diagnostic('warning', 'dynamic-api-name', 'A dynamic Tapp.api name cannot be checked statically', file, location.line, location.column))
      } else if (!manifest.apis || !Object.hasOwn(manifest.apis, name)) {
        diagnostics.push(diagnostic('error', 'undeclared-api', `Tapp.api('${name}') is not declared in manifest.apis`, file, location.line, location.column))
      }
    }

    if (path.length === 2 && path[0] === 'assets' && ASSET_LITERAL_METHODS.has(path[1])) {
      const assetPath = literalString(node.arguments[0])
      if (assetPath !== undefined) {
        const normalized = assetPath.startsWith(`${ASSET_DIRECTORY}/`)
          ? assetPath
          : `${ASSET_DIRECTORY}/${assetPath}`
        const declaredAssets = new Set(Array.isArray(manifest.assets) ? manifest.assets : [])
        if (!declaredAssets.has(normalized)) {
          diagnostics.push(diagnostic('error', 'undeclared-asset', `${normalized} is not declared in manifest.assets`, file, location.line, location.column))
        }
      }
    }

    ts.forEachChild(node, visit)
  }

  visit(sourceFile)
}

async function walkCodeFiles(root, current = root) {
  const files = []
  for (const entry of await readdir(current, { withFileTypes: true })) {
    if (entry.isDirectory() && (SKIP_DIRECTORIES.has(entry.name) || entry.name === 'types')) continue
    const absolute = join(current, entry.name)
    if (entry.isDirectory()) files.push(...(await walkCodeFiles(root, absolute)))
    else if (entry.isFile() && CODE_EXTENSIONS.has(extname(entry.name)) && !entry.name.endsWith('.d.ts')) {
      files.push(absolute)
    }
  }
  return files
}

export async function analyzeProjectCode({ root, manifest, surfaces }) {
  const diagnostics = []
  const requiredPermissions = new Map()
  const usedActions = []
  for (const absolute of await walkCodeFiles(root)) {
    const file = relative(root, absolute).split(sep).join('/')
    const source = await readFile(absolute, 'utf8')
    // Minified engine IIFEs (Three.js, etc.) are too large for the TS AST
    // and must not produce loader/fetch false positives.
    if (source.length > 128 * 1024) {
      warnRemoteEngineScript(source.slice(0, 32 * 1024), file, diagnostics)
      continue
    }
    inspectCode(
      source,
      file,
      manifest,
      diagnostics,
      requiredPermissions,
      usedActions,
      surfaces,
    )
  }
  for (const absolute of await walkMarkupFiles(root)) {
    const file = relative(root, absolute).split(sep).join('/')
    warnRemoteEngineScript(await readFile(absolute, 'utf8'), file, diagnostics)
  }
  return { diagnostics, requiredPermissions, usedActions }
}

async function walkMarkupFiles(root, current = root) {
  const files = []
  for (const entry of await readdir(current, { withFileTypes: true })) {
    if (entry.isDirectory() && (SKIP_DIRECTORIES.has(entry.name) || entry.name === 'types')) continue
    const absolute = join(current, entry.name)
    if (entry.isDirectory()) files.push(...(await walkMarkupFiles(root, absolute)))
    else if (entry.isFile() && extname(entry.name) === '.html') files.push(absolute)
  }
  return files
}
