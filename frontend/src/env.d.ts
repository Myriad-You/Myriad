/// <reference path="../.astro/types.d.ts" />

/** Vite define */
declare const __APP_VERSION__: string

declare module '*?raw' {
  const content: string
  export default content
}

interface ImportMetaEnv {
  readonly PUBLIC_API_URL: string
  readonly DEV: boolean
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
