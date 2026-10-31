import { existsSync } from 'node:fs'
import { resolve } from 'node:path'

/** The Tapp source tree: the contract crates, the exporter and the shared runtime. */
function isTappRoot(root) {
  return (
    existsSync(resolve(root, 'tools/tapp-contract-export/Cargo.toml')) &&
    existsSync(resolve(root, 'runtime/src/permissionConfig.ts')) &&
    existsSync(resolve(root, 'runtime/src/sandbox/capabilityProfiles.ts'))
  )
}

/**
 * Locate the Tapp source tree from inside it, from tapp-store, or from
 * MYRIAD_REPO_ROOT (a Myriad checkout or its tapp/ directory).
 */
export function findTappRoot(packageRoot) {
  const env = process.env.MYRIAD_REPO_ROOT
  const candidates = [
    env,
    env && resolve(env, 'tapp'),
    resolve(packageRoot, '../..'),
    resolve(packageRoot, '../../Myriad/tapp'),
  ].filter(Boolean)

  return candidates.find(isTappRoot)
}
