// Holds the site and @myriad/figure to @myriad-you/figure-interface, with the type checker:
//
// 1. The site against the interface: the site's code, with @myriad/figure replaced by the
//    interface's declarations, type-checks, and nothing in it reaches the implementation's files
//    (an entry the interface does not declare, or a relative path into figure/). Production code
//    must be clean; tests and browser fixtures may not gain an error the real figure does not give
//    them, except those listed below as testing one implementation.
// 2. The implementation against the interface: every name each interface entry declares is in the
//    same @myriad/figure entry; values fit (parameters compare both ways, as methods' do), data
//    types are interchangeable, and classes and types tagged @behaviour (which keep only the members
//    the site uses) are ones figure's stands in for. The list is read from the interface, not kept here.
// 3. Both read the same @myriad-you/figure-player: the one @myriad/figure pins.
import { realpathSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const frontend = path.resolve(fileURLToPath(new URL('../..', import.meta.url)))
const figureRoot = realpathSync(path.resolve(frontend, '../figure'))
const ifaceRoot = path.join(frontend, 'packages/figure-interface')
const ifaceSrc = path.join(ifaceRoot, 'src')

// Tests that drive one implementation's insides (its testing entry, runtime internals, the rig
// importer). They belong with that implementation's own suite and are not held to the interface.
const IMPLEMENTATION_SPECIFIC = new Set([
  'src/features/merope/assets/compiler.test.ts', // figure's rig compiler through @myriad/figure/testing
  'src/features/merope/turnKeyformsApi.test.ts', // figure's importer memory through @myriad/figure/testing
  'src/features/merope/body/anime25dAdapter.test.ts', // builds MotionRuntime itself and reads its frames
  'src/features/merope/motion/playbackDirection.test.ts', // reads speech and performance sources' internals
  'src/features/merope/presence/performancePipeline.test.ts', // ditto
  'src/features/merope/presence/touchAppraisalHost.test.ts', // builds TouchMotionSource itself
  'tests/browser/fixture/main.tsx', // figure's SpeechMotionSource through @myriad/figure/testing
  'tests/browser/fixture/memoryGpu.ts', // figure's PSD importer and Anime25DPlayer.loadAtlas
  'tests/browser/fixture/touchHandoff.tsx', // figure's runtime parts through @myriad/figure/testing
])

const failures = []
const fail = message => failures.push(message)
const rel = file => path.relative(frontend, file)

const config = ts.getParsedCommandLineOfConfigFile(path.join(frontend, 'tsconfig.json'), {}, { ...ts.sys, onUnRecoverableConfigFileDiagnostic: d => fail(ts.flattenDiagnosticMessageText(d.messageText, '\n')) })
const ifaceOptions = {
  ...config.options,
  paths: {
    ...config.options.paths,
    '@myriad/figure': [path.join(ifaceSrc, 'index.d.ts')],
    '@myriad/figure/*': [path.join(ifaceSrc, '*.d.ts')],
  },
}
const production = config.fileNames
const testFiles = ts.sys.readDirectory(path.join(frontend, 'src'), ['.ts', '.tsx'])
  .filter(file => /\.test\.tsx?$/.test(file))
  .concat(ts.sys.readDirectory(path.join(frontend, 'tests/browser/fixture'), ['.ts', '.tsx']))
  .filter(file => !IMPLEMENTATION_SPECIFIC.has(rel(file)))

function diagnostics(program, files) {
  const wanted = new Set(files.map(file => path.resolve(file)))
  return ts.getPreEmitDiagnostics(program).filter(d => d.file && wanted.has(path.resolve(d.file.fileName)))
}
const key = d => `${rel(d.file.fileName)}:${d.start}:${d.code}`
function show(d) {
  const { line, character } = d.file.getLineAndCharacterOfPosition(d.start)
  return `${rel(d.file.fileName)}(${line + 1},${character + 1}): TS${d.code} ${ts.flattenDiagnosticMessageText(d.messageText, ' ')}`
}

// Every module a site file names that resolves into the implementation.
function reachesImplementation(program, options, files) {
  for (const file of files) {
    const source = program.getSourceFile(file)
    if (!source) continue
    const specifiers = []
    const visit = (node) => {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteral(node.moduleSpecifier)) specifiers.push(node.moduleSpecifier.text)
      if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) specifiers.push(node.arguments[0].text)
      if (ts.isImportTypeNode(node) && ts.isLiteralTypeNode(node.argument) && ts.isStringLiteral(node.argument.literal)) specifiers.push(node.argument.literal.text)
      ts.forEachChild(node, visit)
    }
    visit(source)
    for (const specifier of specifiers) {
      const resolved = ts.resolveModuleName(specifier, file, options, ts.sys).resolvedModule
      if (!resolved) continue
      let target = resolved.resolvedFileName
      try { target = realpathSync(target) }
      catch {}
      if (target.startsWith(`${figureRoot}${path.sep}`)) fail(`${rel(file)} reaches the implementation through '${specifier}' (${path.relative(frontend, target)})`)
    }
  }
}

// Declaration files (the libraries, the renderer, the interface) are parsed once for every program.
const shared = new Map()
function cachingHost(options) {
  const host = ts.createCompilerHost(options)
  const getSourceFile = host.getSourceFile.bind(host)
  host.getSourceFile = (file, version, onError, create) => {
    if (!file.endsWith('.d.ts')) return getSourceFile(file, version, onError, create)
    const id = `${file}|${typeof version === 'object' ? version.languageVersion : version}`
    if (!shared.has(id)) shared.set(id, getSourceFile(file, version, onError, create))
    return shared.get(id)
  }
  return host
}

// 1. The site against the interface. Tests are compared with what the real figure gives them, so
// that program goes first and is let go before the next.
const alreadyFailing = (() => {
  const program = ts.createProgram(testFiles, config.options, cachingHost(config.options))
  return new Set(diagnostics(program, testFiles).map(key))
})()
{
  const program = ts.createProgram([...production, ...testFiles], ifaceOptions, cachingHost(ifaceOptions))
  for (const d of diagnostics(program, production)) fail(show(d))
  for (const d of diagnostics(program, testFiles)) {
    if (!alreadyFailing.has(key(d))) fail(show(d))
  }
  reachesImplementation(program, ifaceOptions, [...production, ...testFiles])
}

// 2. The implementation against the interface.
const entries = ts.sys.readDirectory(ifaceSrc, ['.d.ts']).filter(file => !file.endsWith('declarations.d.ts')).toSorted()
const entryProgram = ts.createProgram(entries, config.options, cachingHost(config.options))
const checker = entryProgram.getTypeChecker()
const alias = file => path.basename(file, '.d.ts').replace(/^./, c => c.toUpperCase())
const specifier = (pkg, file) => (path.basename(file) === 'index.d.ts' ? pkg : `${pkg}/${path.basename(file, '.d.ts')}`)
const lines = []
const rows = []
for (const file of entries) {
  const name = alias(file)
  lines.push(`import type * as Figure${name} from '${specifier('@myriad/figure', file)}'`)
  lines.push(`import type * as Interface${name} from '${specifier('@myriad-you/figure-interface', file)}'`)
  let values = false
  for (const exported of checker.getExportsOfModule(checker.getSymbolAtLocation(entryProgram.getSourceFile(file)))) {
    const spec = exported.declarations?.[0]
    const typeOnly = !!spec && ts.isExportSpecifier(spec) && (spec.isTypeOnly || spec.parent.parent.isTypeOnly)
    const target = exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported
    if (!typeOnly && target.flags & ts.SymbolFlags.Value) values = true
    if (!(target.flags & ts.SymbolFlags.Type)) continue
    // A class is compared one way: figure's has private state the declaration leaves out.
    const behaviour = (target.declarations ?? []).some(d => ts.isClassDeclaration(d) || ts.getJSDocTags(d).some(tag => tag.tagName.text === 'behaviour'))
    const generic = (target.declarations ?? []).some(d => (ts.isTypeAliasDeclaration(d) || ts.isInterfaceDeclaration(d) || ts.isClassDeclaration(d)) && d.typeParameters?.some(p => !p.default))
    if (generic) { fail(`${rel(file)}: ${exported.name} takes type arguments; the check compares types used without them`); continue }
    rows.push(`  Provides<Figure${name}.${exported.name}, Interface${name}.${exported.name}>,`)
    if (!behaviour) rows.push(`  Accepts<Interface${name}.${exported.name}, Figure${name}.${exported.name}>,`)
  }
  if (values) rows.push(`  Provides<typeof Figure${name}, Values<typeof Interface${name}>>,`)
}
const satisfiesFile = path.join(frontend, 'src/__figure_satisfies_interface__.ts')
const satisfiesSource = `${lines.join('\n')}
type Bivariant<T> = T extends (...args: infer A) => infer R ? { method(...args: A): R }['method'] : T
type Values<M> = { [K in keyof M]: Bivariant<M[K]> }
type Provides<Impl extends Iface, Iface> = [Impl, Iface]
type Accepts<Iface extends Impl, Impl> = [Iface, Impl]
export type FigureProvidesInterface = [
${rows.join('\n')}
]
`
const host = cachingHost(config.options)
const readFile = host.readFile.bind(host)
const getSourceFile = host.getSourceFile.bind(host)
host.readFile = file => (path.resolve(file) === satisfiesFile ? satisfiesSource : readFile(file))
host.fileExists = (exists => file => path.resolve(file) === satisfiesFile || exists(file))(host.fileExists.bind(host))
host.getSourceFile = (file, version, onError, create) => (path.resolve(file) === satisfiesFile
  ? ts.createSourceFile(file, satisfiesSource, version, true)
  : getSourceFile(file, version, onError, create))
const satisfiesProgram = ts.createProgram([satisfiesFile], config.options, host)
for (const d of diagnostics(satisfiesProgram, [satisfiesFile])) fail(`figure does not provide the interface: ${ts.flattenDiagnosticMessageText(d.messageText, ' ')}`)

// 3. One renderer.
function playerFrom(dir) {
  try { return realpathSync(createRequire(path.join(dir, 'package.json')).resolve('@myriad-you/figure-player/package.json')) }
  catch { return null }
}
const forFigure = playerFrom(figureRoot)
const forInterface = playerFrom(ifaceRoot)
if (!forFigure || forFigure !== forInterface) fail(`the interface reads ${forInterface ?? 'no'} @myriad-you/figure-player, figure pins ${forFigure ?? 'none'}`)

const checked = `${production.length} production files, ${testFiles.length} tests and fixtures, ${rows.length} interface comparisons`
if (failures.length) {
  console.error(failures.join('\n'))
  console.error(`figure interface: ${failures.length} problem(s) (${checked})`)
  process.exit(1)
}
console.log(`figure interface: ok (${checked})`)
