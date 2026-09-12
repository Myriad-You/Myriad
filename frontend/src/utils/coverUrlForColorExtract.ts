/** Resize the upstream URL inside /api/proxy/image (CORS). */
export function coverUrlForColorExtract(url: string): string {
  if (!url) return url
  try {
    let upstream = url
    let proxied = false
    const proxyMatch = url.match(/\/api\/proxy\/image\?url=([^&]+)/i)
    if (proxyMatch) {
      try {
        upstream = decodeURIComponent(proxyMatch[1])
        proxied = true
      } catch {
        return url
      }
    }

    let resized = upstream
    if (
      /music\.(126|163)\.(net|com)/i.test(upstream) ||
      /p\d+\.music\.126\.net/i.test(upstream)
    ) {
      if (/[?&]param=\d+y\d+/i.test(upstream)) {
        resized = upstream.replaceAll(/([?&]param=)\d+y\d+/ig, '$1150y150')
      } else {
        resized = upstream.includes('?')
          ? `${upstream}&param=150y150`
          : `${upstream}?param=150y150`
      }
    } else if (
      /y\.gtimg\.cn\/music\/photo_new\/T002R\d+x\d+M000/i.test(upstream)
    ) {
      resized = upstream.replaceAll(/T002R\d+x\d+M000/ig, 'T002R150x150M000')
    } else if (!proxied) {
      return url
    }

    if (!proxied) return resized
    const prefix = url.slice(0, url.indexOf('url='))
    return `${prefix}url=${encodeURIComponent(resized)}`
  } catch {
  }
  return url
}
