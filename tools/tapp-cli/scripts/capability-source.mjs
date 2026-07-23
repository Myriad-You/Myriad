import ts from 'typescript'

function unwrap(node) {
  while (ts.isAsExpression(node) || ts.isSatisfiesExpression(node)) node = node.expression
  return node
}

function variableInitializer(sourceFile, name) {
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

function literalProfiles(type) {
  if (ts.isUnionTypeNode(type)) return type.types.flatMap(literalProfiles)
  return ts.isLiteralTypeNode(type) && ts.isStringLiteral(type.literal)
    ? [type.literal.text]
    : []
}

function requireParseable(sourceFile) {
  if (sourceFile.parseDiagnostics.length > 0) {
    throw new Error('capabilityProfiles.ts contains TypeScript syntax errors')
  }
}

export function parseCapabilitySource(source) {
  const sourceFile = ts.createSourceFile(
    'capabilityProfiles.ts',
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  )
  requireParseable(sourceFile)
  const denied = unwrap(variableInitializer(sourceFile, 'HEADLESS_DENIED_ACTIONS'))
  if (!ts.isArrayLiteralExpression(denied)) {
    throw new Error('HEADLESS_DENIED_ACTIONS must be an array literal')
  }
  const headlessDeniedActions = denied.elements.map((element) => {
    const value = unwrap(element)
    if (!ts.isStringLiteralLike(value)) throw new Error('HEADLESS_DENIED_ACTIONS must contain strings')
    return value.text
  })
  if (headlessDeniedActions.length < 10) {
    throw new Error('Capability profile parse produced unexpectedly few denied actions')
  }

  const profile = sourceFile.statements.find(
    (statement) => ts.isTypeAliasDeclaration(statement) && statement.name.text === 'SandboxCapabilityProfile',
  )
  const profiles = profile ? literalProfiles(profile.type) : []
  if (profiles.length === 0) {
    throw new Error('Unable to locate SandboxCapabilityProfile literals')
  }

  return { profiles, headlessDeniedActions }
}
