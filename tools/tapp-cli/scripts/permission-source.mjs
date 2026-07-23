import ts from 'typescript'

function initializer(sourceFile, name) {
  for (const statement of sourceFile.statements) {
    if (!ts.isVariableStatement(statement)) continue
    for (const declaration of statement.declarationList.declarations) {
      if (ts.isIdentifier(declaration.name) && declaration.name.text === name) {
        return declaration.initializer
      }
    }
  }
  throw new Error(`Unable to locate ${name}`)
}

function unwrap(node) {
  while (ts.isAsExpression(node) || ts.isSatisfiesExpression(node)) node = node.expression
  return node
}

function stringValue(node) {
  node = unwrap(node)
  return ts.isStringLiteralLike(node) ? node.text : undefined
}

function propertyName(node) {
  return ts.isIdentifier(node) || ts.isStringLiteralLike(node) ? node.text : undefined
}

function requireParseable(sourceFile) {
  if (sourceFile.parseDiagnostics.length > 0) {
    throw new Error('permissionConfig.ts contains TypeScript syntax errors')
  }
}

export function parsePermissionSource(source) {
  const sourceFile = ts.createSourceFile(
    'permissionConfig.ts',
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  )
  requireParseable(sourceFile)
  const levels = unwrap(initializer(sourceFile, 'PERMISSION_LEVELS'))
  if (!ts.isObjectLiteralExpression(levels)) {
    throw new Error('PERMISSION_LEVELS must be an object literal')
  }

  const permissionLevels = {}
  for (const property of levels.properties) {
    if (!ts.isPropertyAssignment(property)) continue
    const name = propertyName(property.name)
    const value = stringValue(property.initializer)
    if (name && value !== undefined) permissionLevels[name] = value
  }

  const permissionMap = unwrap(initializer(sourceFile, 'PERMISSION_MAP'))
  if (!ts.isNewExpression(permissionMap) || !ts.isIdentifier(permissionMap.expression) || permissionMap.expression.text !== 'Map') {
    throw new Error('PERMISSION_MAP must be a Map literal')
  }
  const entries = unwrap(permissionMap.arguments?.[0])
  if (!ts.isArrayLiteralExpression(entries)) {
    throw new Error('PERMISSION_MAP must contain an entry array')
  }

  const actions = {}
  for (const entry of entries.elements) {
    if (!ts.isArrayLiteralExpression(entry) || entry.elements.length !== 2) continue
    const action = stringValue(entry.elements[0])
    const permission = stringValue(entry.elements[1])
    if (action !== undefined && permission !== undefined) actions[action] = permission
  }

  if (Object.keys(permissionLevels).length < 30 || Object.keys(actions).length < 150) {
    throw new Error('Permission catalog parse produced unexpectedly few entries')
  }

  return { permissionLevels, actions }
}
