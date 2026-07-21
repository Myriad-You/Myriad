/**
 * Resolve store-hosted package asset URLs (shared by RemoteStoreService).
 *
 * Package root = parent of main.js / manifest.json on the store host.
 * Asset URL relative path = `{packageRoot}/{assetPath}`.
 */

/** Package dir on store host: parent of main.js / manifest.json. */
export function storePackageRoot(codeOrManifestPath: string): string {
  const path = codeOrManifestPath.trim().replace(/^\/+/, '')
  const i = path.lastIndexOf('/')
  return i >= 0 ? path.slice(0, i) : ''
}

/** Store-relative path for a package asset. */
export function storeAssetStorePath(
  packageRoot: string,
  assetPath: string,
): string {
  const asset = assetPath.trim().replace(/^\/+/, '')
  const root = packageRoot.trim().replace(/^\/+|\/+$/g, '')
  return root ? `${root}/${asset}` : asset
}
