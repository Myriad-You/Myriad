/// <reference path="../.astro/types.d.ts" />
/// <reference types="astro/client" />

// 全局常量（由 Vite define 注入）
declare const __APP_VERSION__: string

interface ImportMetaEnv {
  readonly PUBLIC_API_URL: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
