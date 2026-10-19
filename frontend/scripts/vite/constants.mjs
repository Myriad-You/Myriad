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

/** 创作台的嵌入页用自己的 `studio.html`，不走主站的 `index.html`。 */
export const STUDIO_DOCUMENT = 'studio.html'

export function isStudioDocumentPath(pathname) {
  return pathname === '/embed/studio/overlay' || pathname.startsWith('/embed/studio/tool/')
}

export const BACKEND_TARGET = 'http://127.0.0.1:1103'

export const STATIC_ASSET_PATTERN =
  /\.(?:js|mjs|cjs|css|map|png|webp|jpe?g|gif|svg|ico|woff2?|ttf|otf|json|webmanifest|txt|wasm|webm|mp3|mp4)$/i
