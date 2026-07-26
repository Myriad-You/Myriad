import { access, lstat, realpath, readdir, readFile, stat } from 'node:fs/promises'
import { extname, isAbsolute, join, relative, resolve, sep } from 'node:path'

const contract = JSON.parse(
  await readFile(new URL('./generated/contract.json', import.meta.url), 'utf8'),
)
const MANIFEST_RESOURCE_FIELDS = contract.rules.manifestResourceFields
const AGENT_SCHEMA_FIELDS = new Set(contract.rules.agentSchemaFields)
const PACKAGE_RESOURCE_EXTENSIONS = contract.rules.packageResourceExtensions
const PACKAGE_JSON_OBJECT_DIRECTORIES = new Set(contract.rules.packageJsonObjectDirectories)
const PACKAGE_RESOURCE_FILE_LIMITS = contract.rules.packageResourceFileLimits
const PACKAGE_RESOURCE_BYTE_LIMITS = contract.rules.packageResourceByteLimits
const ASSET_DIRECTORY = contract.rules.assetDirectory
const PAGE_MODULE_DIRECTORY = contract.rules.pageModuleDirectory
const SAFE_COMPONENT = new RegExp(contract.patterns.safeComponent)

function diagnostic(severity, code, message, file = 'manifest.json', line, column) {
  return { severity, code, message, file, line, column }
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function arrayValue(value) {
  return Array.isArray(value) ? value : []
}

function formatBytes(bytes) {
  for (const [unitBytes, unit] of [[1024 ** 3, 'GiB'], [1024 ** 2, 'MiB'], [1024, 'KiB']]) {
    if (bytes >= unitBytes && bytes % unitBytes === 0) return `${bytes / unitBytes} ${unit}`
  }
  return `${bytes} bytes`
}

function validateInlineSchema(schema) {
  if (!isObject(schema) || Buffer.byteLength(JSON.stringify(schema)) > contract.limits.dataExchangeSchemaBytes) return false
  if (!contract.rules.inlineSchemaRootKeys.some((field) => field in schema)) return false
  const visit = (value, depth) => {
    if (depth > contract.limits.inlineSchemaDepth) return false
    if (Array.isArray(value)) return value.every((child) => visit(child, depth + 1))
    if (!isObject(value)) return true
    if ('$ref' in value) return false
    return Object.values(value).every((child) => visit(child, depth + 1))
  }
  return visit(schema, 0)
}

function validateResourcePath(value) {
  if (typeof value !== 'string' || !value || value.length > contract.limits.resourcePathLength || value.includes('\\')) return false
  if (value.startsWith('/') || /^[A-Za-z]:/.test(value)) return false
  return value.split('/').every((component) => component.length <= contract.limits.tappIdLength && component !== '.' && component !== '..' && SAFE_COMPONENT.test(component))
}

function isPathWithin(root, target) {
  const path = relative(root, target)
  return path !== '' && path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path)
}

async function pathExists(path) {
  try {
    await access(path)
    return true
  } catch {
    return false
  }
}

function resourceDeclarations(manifest) {
  const declarations = new Map()
  const add = (path, kind, extension) => {
    if (typeof path === 'string' && path) declarations.set(path, { kind, extension })
  }
  for (const [field, extensionKey] of Object.entries(MANIFEST_RESOURCE_FIELDS)) {
    add(manifest[field], field, contract.rules.resourceExtensions[extensionKey])
  }
  for (const module of arrayValue(manifest.pageModules)) add(`${PAGE_MODULE_DIRECTORY}/${module}`, 'pageModule', contract.rules.resourceExtensions.pageModule)
  for (const asset of arrayValue(manifest.assets)) add(asset, 'asset')
  for (const widget of arrayValue(manifest.widgets)) {
    if (!isObject(widget)) continue
    for (const path of Object.values(widget.templates || {})) add(path, `widgetTemplate:${widget.id}`, contract.rules.resourceExtensions.widgetTemplate)
  }
  for (const interaction of arrayValue(manifest.agent?.interactions)) {
    for (const field of AGENT_SCHEMA_FIELDS) add(interaction?.[field], 'agentSchema', contract.rules.resourceExtensions.agentSchema)
  }
  return declarations
}

export async function validateProjectResources(root, manifest) {
  const diagnostics = []
  const declarations = resourceDeclarations(manifest)
  const packagePaths = new Set(['manifest.json'])
  const canonicalRoot = await realpath(root)
  let assetBytes = 0

  for (const [path, metadata] of declarations) {
    if (!validateResourcePath(path)) {
      diagnostics.push(diagnostic('error', 'invalid-resource-path', `Invalid ${metadata.kind} path: ${path}`))
      continue
    }
    if (metadata.extension && extname(path) !== metadata.extension) {
      diagnostics.push(diagnostic('error', 'invalid-resource-extension', `${metadata.kind} must use ${metadata.extension}: ${path}`))
    }
    const absolute = resolve(root, path)
    if (!isPathWithin(root, absolute)) {
      diagnostics.push(diagnostic('error', 'invalid-resource-path', `Resource escapes the project root: ${path}`))
      continue
    }
    if (!(await pathExists(absolute))) {
      diagnostics.push(diagnostic('error', 'missing-resource', `Declared resource not found: ${path}`))
      continue
    }
    const info = await lstat(absolute)
    if (!info.isFile()) {
      diagnostics.push(diagnostic('error', 'invalid-resource', `Resource is not a file: ${path}`))
      continue
    }
    if (!isPathWithin(canonicalRoot, await realpath(absolute))) {
      diagnostics.push(diagnostic('error', 'invalid-resource', `Resource escapes the project root: ${path}`))
      continue
    }
    packagePaths.add(path)
    if (info.size > contract.limits.resourceBytes) {
      diagnostics.push(diagnostic('error', 'resource-too-large', `${path} exceeds ${formatBytes(contract.limits.resourceBytes)}`))
    }
    if (metadata.kind === 'asset') {
      assetBytes += info.size
      if (!path.startsWith(`${ASSET_DIRECTORY}/`) || contract.rules.assetForbiddenExtensions.includes(extname(path))) {
        diagnostics.push(diagnostic('error', 'invalid-asset', `Asset must be under ${ASSET_DIRECTORY}/ and cannot be JS/HTML: ${path}`))
      }
      if (info.size > contract.limits.assetBytes) diagnostics.push(diagnostic('error', 'asset-too-large', `Asset exceeds ${formatBytes(contract.limits.assetBytes)}: ${path}`))
    }
    if (metadata.kind === 'agentSchema') {
      if (info.size > contract.limits.agentSchemaBytes) diagnostics.push(diagnostic('error', 'agent-schema-too-large', `${path} exceeds ${formatBytes(contract.limits.agentSchemaBytes)}`))
      try {
        const schema = JSON.parse(await readFile(absolute, 'utf8'))
        if (!validateInlineSchema(schema)) throw new Error('unsupported schema')
      } catch {
        diagnostics.push(diagnostic('error', 'invalid-agent-schema', `${path} is not a supported inline JSON Schema`))
      }
    }
  }

  if (arrayValue(manifest.assets).length > contract.limits.assets) diagnostics.push(diagnostic('error', 'too-many-assets', `assets accepts at most ${contract.limits.assets} entries`))
  if (assetBytes > contract.limits.assetsTotalBytes) diagnostics.push(diagnostic('error', 'assets-too-large', `Declared assets exceed ${formatBytes(contract.limits.assetsTotalBytes)} total`))

  for (const [directory, extension] of Object.entries(PACKAGE_RESOURCE_EXTENSIONS)) {
    const absoluteDirectory = join(root, directory)
    let directoryInfo
    try {
      directoryInfo = await lstat(absoluteDirectory)
    } catch (error) {
      if (error.code === 'ENOENT') continue
      diagnostics.push(diagnostic('error', `invalid-${directory}`, `${directory} directory cannot be read`))
      continue
    }
    // Reject symlinked directories so packed files cannot escape the project root.
    if (!directoryInfo.isDirectory()) {
      diagnostics.push(diagnostic('error', `invalid-${directory}`, `${directory} must be a regular directory inside the project`))
      continue
    }
    let directoryEntries
    try {
      directoryEntries = await readdir(absoluteDirectory, { withFileTypes: true })
    } catch {
      diagnostics.push(diagnostic('error', `invalid-${directory}`, `${directory} directory cannot be read`))
      continue
    }
    const fileLimitKey = PACKAGE_RESOURCE_FILE_LIMITS[directory]
    if (fileLimitKey && directoryEntries.length > contract.limits[fileLimitKey]) diagnostics.push(diagnostic('error', `too-many-${directory}-files`, `${directory} accepts at most ${contract.limits[fileLimitKey]} files`))
    for (const entry of directoryEntries) {
      if (PACKAGE_JSON_OBJECT_DIRECTORIES.has(directory) && (!entry.isFile() || extname(entry.name) !== extension)) {
        diagnostics.push(diagnostic('error', `invalid-${directory}`, `${directory}/${entry.name} must be a regular JSON file`))
        continue
      }
      if (!entry.isFile()) continue
      if (!PACKAGE_JSON_OBJECT_DIRECTORIES.has(directory) && extname(entry.name) !== extension) continue
      const path = `${directory}/${entry.name}`
      if (!validateResourcePath(path)) {
        diagnostics.push(diagnostic('error', 'invalid-resource-path', `Invalid path: ${path}`))
        continue
      }
      // Dirent.isFile() uses lstat semantics and the parent directory is a
      // verified non-symlink, so entries here cannot escape the project root.
      const absoluteEntry = join(absoluteDirectory, entry.name)
      let info
      try {
        info = await stat(absoluteEntry)
      } catch {
        diagnostics.push(diagnostic('error', `invalid-${directory}`, `${path} cannot be read`))
        continue
      }
      const byteLimitKey = PACKAGE_RESOURCE_BYTE_LIMITS[directory]
      const directoryLimit = byteLimitKey ? contract.limits[byteLimitKey] : Number.POSITIVE_INFINITY
      const byteLimit = Math.min(directoryLimit, contract.limits.resourceBytes)
      if (info.size > byteLimit) {
        const code = byteLimit === directoryLimit ? `${directory}-too-large` : 'resource-too-large'
        diagnostics.push(diagnostic('error', code, `${path} exceeds ${formatBytes(byteLimit)}`))
      }
      if (PACKAGE_JSON_OBJECT_DIRECTORIES.has(directory)) {
        try {
          const value = JSON.parse(await readFile(absoluteEntry, 'utf8'))
          if (!isObject(value)) throw new Error('resource must be an object')
        } catch {
          diagnostics.push(diagnostic('error', `invalid-${directory}`, `${path} must contain a JSON object`))
        }
      }
      packagePaths.add(path)
    }
  }
  return { diagnostics, packageFiles: [...packagePaths].sort() }
}
