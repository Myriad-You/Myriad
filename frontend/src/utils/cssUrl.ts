export function cssUrl(url: string | null | undefined): string {
  if (url == null) return 'none'
  const s = String(url)
  if (!s) return 'none'

  const escaped = s
    .replaceAll('\\', '\\\\')
    .replaceAll('"', '\\"')
    .replaceAll('\n', '\\A ')
    .replaceAll('\r', '')
    .replaceAll('\f', '')

  return `url("${escaped}")`
}

export function cssBackgroundImage(url: string | null | undefined): string {
  return cssUrl(url)
}
