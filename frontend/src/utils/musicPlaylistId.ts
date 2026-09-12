export function normalizeMusicPlaylistId(raw: string): string {
  const s = raw.trim()
  if (!s) return ''
  if (/^\d+$/.test(s)) return s

  const idEq = s.match(/[?&#]id=(\d+)/i) ?? s.match(/id=(\d+)/i)
  if (idEq?.[1]) return idEq[1]

  const path = s.match(/\/playlist\/(\d+)/i)
  if (path?.[1]) return path[1]

  const runs = s.match(/\d{5,}/g)
  if (runs?.length) {
    return runs.reduce((a, b) => (b.length > a.length ? b : a), runs[0])
  }
  return s
}
