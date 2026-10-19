/// <reference types="vite/client" />

/** Vite define */
declare const __APP_VERSION__: string
/** 创作台嵌入页单独构建的那一遍（studio.html）为 true。 */
declare const __STUDIO_PASS__: boolean

declare module '*?raw' {
  const content: string
  export default content
}

/** Prism 语法组件只有副作用（往全局 Prism 注册），没有导出。 */
declare module 'prismjs/components/prism-*' {
  const nothing: undefined
  export default nothing
}

/** KaTeX 附加包只有副作用，没有类型。 */
declare module 'katex/contrib/mhchem' {
  const nothing: undefined
  export default nothing
}

declare module 'katex/contrib/copy-tex' {
  const nothing: undefined
  export default nothing
}

interface ImportMetaEnv {
  readonly PUBLIC_API_URL: string
  readonly DEV: boolean
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
