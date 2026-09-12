export function storePackageRoot(codeOrManifestPath: string): string {
  const path = codeOrManifestPath.trim().replaceAll(/^\/+/g, '')
  const i = path.lastIndexOf('/')
  return i >= 0 ? path.slice(0, i) : ''
}

export function storeAssetStorePath(
  packageRoot: string,
  assetPath: string,
): string {
  const asset = assetPath.trim().replaceAll(/^\/+/g, '')
  const root = packageRoot.trim().replaceAll(/^\/+|\/+$/g, '')
  return root ? `${root}/${asset}` : asset
}
