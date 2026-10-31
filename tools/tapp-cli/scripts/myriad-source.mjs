import { existsSync } from 'node:fs'
import { resolve } from 'node:path'

function isMyriadRoot(root) {
  return (
    existsSync(resolve(root, 'tools/tapp-contract-export/Cargo.toml')) &&
    existsSync(resolve(root, 'tapp/runtime/src/permissionConfig.ts')) &&
    existsSync(
      resolve(
        root,
        'tapp/runtime/src/sandbox/capabilityProfiles.ts',
      ),
    )
  )
}

/** Locate the upstream Myriad source from either the monorepo or tapp-store. */
export function findMyriadRepoRoot(packageRoot) {
  const candidates = [
    process.env.MYRIAD_REPO_ROOT,
    resolve(packageRoot, '../..'),
    resolve(packageRoot, '../../Myriad'),
  ].filter(Boolean)

  return candidates.find(isMyriadRoot)
}
