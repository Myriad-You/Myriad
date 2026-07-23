export function parsePermissionSource(source) {
  const levelsBlock = source.match(
    /export const PERMISSION_LEVELS[\s\S]*?=\s*\{([\s\S]*?)\n\}/,
  )
  if (!levelsBlock) throw new Error('Unable to locate PERMISSION_LEVELS')

  const permissionLevels = {}
  for (const match of levelsBlock[1].matchAll(
    /(?:'([^']+)'|([A-Za-z][\w-]*))\s*:\s*'([^']+)'/g,
  )) {
    permissionLevels[match[1] || match[2]] = match[3]
  }

  const mapBlock = source.match(
    /export const PERMISSION_MAP[\s\S]*?new Map\(\[([\s\S]*?)\n\s*\]\)/,
  )
  if (!mapBlock) throw new Error('Unable to locate PERMISSION_MAP')

  const actions = {}
  for (const match of mapBlock[1].matchAll(/\['([^']+)',\s*'([^']+)'\]/g)) {
    actions[match[1]] = match[2]
  }

  if (Object.keys(permissionLevels).length < 30 || Object.keys(actions).length < 150) {
    throw new Error('Permission catalog parse produced unexpectedly few entries')
  }

  return { permissionLevels, actions }
}
