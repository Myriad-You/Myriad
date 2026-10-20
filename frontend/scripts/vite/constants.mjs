/** Align with proxy/backend: document-level geolocation for weather. */
export const DOCUMENT_PERMISSIONS_POLICY =
  'geolocation=(self), microphone=(self), camera=()'

/** `/embed/*` documents are framed by `/blog` pages on this origin and nowhere else. */
export const EMBED_FRAME_HEADERS = Object.freeze({
  'Content-Security-Policy': "frame-ancestors 'self'",
  'X-Frame-Options': 'SAMEORIGIN',
})

export function isEmbedDocumentPath(pathname) {
  return pathname === '/embed' || pathname.startsWith('/embed/')
}

/** 嵌入页（伴读、评论、友链申请、创作台）用自己的 `embed.html`，不走主站的 `index.html`。 */
export const EMBED_DOCUMENT = 'embed.html'

export const BACKEND_TARGET = 'http://127.0.0.1:1103'

export const STATIC_ASSET_PATTERN =
  /\.(?:js|mjs|cjs|css|map|png|webp|jpe?g|gif|svg|ico|woff2?|ttf|otf|json|webmanifest|txt|wasm|webm|mp3|mp4)$/i
