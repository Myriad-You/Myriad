import { execFile } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { promisify } from 'node:util'
import { fileURLToPath } from 'node:url'
import { parseCapabilitySource } from './capability-source.mjs'
import { parsePermissionSource } from './permission-source.mjs'
import { generateTappSdkDts } from './sdk-dts.mjs'

const execFileAsync = promisify(execFile)
const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '../../..')
const permissionSourcePath = resolve(
  repoRoot,
  'frontend/src/tapp/runtime/permissionConfig.ts',
)
const capabilitySourcePath = resolve(
  repoRoot,
  'frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts',
)
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
const backendContract = JSON.parse(stdout)
const permissionSource = await readFile(permissionSourcePath, 'utf8')
const { permissionLevels, actions } = parsePermissionSource(permissionSource)
const capabilitySource = await readFile(capabilitySourcePath, 'utf8')
const capabilities = parseCapabilitySource(capabilitySource)

const contract = {
  generatedFrom: [
    'crates/tapp-contract/src/manifest.rs',
    'crates/tapp-contract/src/contract_rules.rs',
    'frontend/src/tapp/runtime/permissionConfig.ts',
    'frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts',
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
