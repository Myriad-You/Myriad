import { execFile } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { promisify } from 'node:util'
import { fileURLToPath } from 'node:url'
import { findMyriadRepoRoot } from './myriad-source.mjs'
import { generateTappSdkDts } from './sdk-dts.mjs'

const execFileAsync = promisify(execFile)
const here = dirname(fileURLToPath(import.meta.url))
const packageRoot = resolve(here, '..')
const repoRoot = findMyriadRepoRoot(packageRoot)
if (!repoRoot) {
  throw new Error(
    'Unable to locate the Myriad source tree; set MYRIAD_REPO_ROOT before syncing the generated contract',
  )
}
const exporterManifestPath = resolve(
  repoRoot,
  'tools/tapp-contract-export/Cargo.toml',
)
const generatedDir = resolve(here, '../src/generated')
const outputPath = resolve(generatedDir, 'contract.json')
const schemaPath = resolve(generatedDir, 'manifest.schema.json')
const capabilityPath = resolve(generatedDir, 'capability-profiles.json')
const sdkDtsPath = resolve(generatedDir, 'tapp-sdk.d.ts')

const { stdout } = await execFileAsync(
  'cargo',
  [
    'run',
    '--quiet',
    '--locked',
    '--manifest-path',
    exporterManifestPath,
  ],
  { cwd: repoRoot, maxBuffer: 4 * 1024 * 1024 },
)
// The neutral contract exporter is the only authority; nothing here reads
// frontend implementation sources.
const { actions, capabilities, ...backendContract } = JSON.parse(stdout)
const permissionLevels = backendContract.permissionLevels
if (!permissionLevels || Object.keys(permissionLevels).length < 30) {
  throw new Error('tapp-contract export did not include permissionLevels')
}
if (!actions || Object.keys(actions).length < 150) {
  throw new Error('tapp-contract export did not include sandbox actions')
}
if (!capabilities?.profiles?.length || !capabilities?.headlessDeniedActions?.length) {
  throw new Error('tapp-contract export did not include capability profiles')
}

const contract = {
  generatedFrom: [
    'crates/tapp-contract/src/manifest.rs',
    'crates/tapp-contract/src/contract_rules.rs',
    'crates/tapp-contract/src/permission.rs',
    'shared/tapp_sandbox_contract.json',
  ],
  ...backendContract,
  permissions: { permissionLevels, actions },
  capabilities,
}

// Keep CLI-facing metadata after the spread so backend schema fields cannot replace it.
const manifestSchema = {
  ...backendContract.schema,
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $id: 'https://myriad.local/tapp/manifest.schema.json',
  title: 'Myriad Tapp Manifest',
  description:
    'Generated from backend TappManifest schema. Semantic limits and permission rules live in contract.json.',
}

const sdkDts = generateTappSdkDts({
  actions,
  headlessDeniedActions: capabilities.headlessDeniedActions,
})

await mkdir(generatedDir, { recursive: true })
await writeFile(outputPath, `${JSON.stringify(contract, null, 2)}\n`)
await writeFile(schemaPath, `${JSON.stringify(manifestSchema, null, 2)}\n`)
await writeFile(capabilityPath, `${JSON.stringify(capabilities, null, 2)}\n`)
await writeFile(sdkDtsPath, sdkDts.endsWith('\n') ? sdkDts : `${sdkDts}\n`)
console.log(
  `Wrote TApp contract with ${Object.keys(actions).length} actions, ${Object.keys(permissionLevels).length} permissions, and ${capabilities.headlessDeniedActions.length} headless-denied actions to ${outputPath}`,
)
console.log(`Wrote Manifest JSON Schema to ${schemaPath}`)
console.log(`Wrote capability profiles to ${capabilityPath}`)
console.log(`Wrote sandbox SDK types to ${sdkDtsPath}`)
