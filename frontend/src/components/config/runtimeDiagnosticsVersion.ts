export const VERSION_TAG_DISPLAY_MAX = 12

export function truncateVersionTag(
  tag: string,
  maxLen: number = VERSION_TAG_DISPLAY_MAX,
): string {
  const value = tag.trim()
  if (maxLen < 1) return ''
  if (value.length <= maxLen) return value
  if (maxLen === 1) return '…'
  return `${value.slice(0, maxLen - 1)}…`
}
