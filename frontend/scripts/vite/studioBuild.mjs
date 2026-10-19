/**
 * 创作台的嵌入页（`studio.html`）单独构建一次。和主站放在同一次构建里，两个入口
 * 共用的初始模块会被 app-shell 合成一块，创作台的每个窗口就又背上了主站外壳；
 * 按入口拆开（entriesAware）又会把主站首页拆成几十个预加载小块。所以主站照旧构建，
 * 完了再只拿 `studio.html` 构建一次，写进同一个目录。
 */

export const STUDIO_PASS_ENV = 'MYRIAD_STUDIO_PASS'

export function isStudioPass() {
  // eslint-disable-next-line node/prefer-global/process
  return process.env[STUDIO_PASS_ENV] === '1'
}

export function studioBuildPlugin() {
  let config
  return {
    name: 'myriad:studio-build',
    apply: 'build',
    configResolved(resolved) {
      config = resolved
    },
    async closeBundle() {
      if (isStudioPass() || config.build.watch) return
      if (this.environment && this.environment.name !== 'client') return
      const { build } = await import('vite')
      // eslint-disable-next-line node/prefer-global/process
      process.env[STUDIO_PASS_ENV] = '1'
      try {
        await build({
          configFile: config.configFile,
          root: config.root,
          mode: config.mode,
          logLevel: config.logLevel,
          build: { outDir: config.build.outDir, emptyOutDir: false },
        })
      } finally {
        // eslint-disable-next-line node/prefer-global/process
        delete process.env[STUDIO_PASS_ENV]
      }
    },
  }
}

/**
 * 笔记编辑器里按名字找图标的 `lib/iconLookup.ts` 引用了近百个品牌图标。react-icons
 * 每套图标是一个模块，摇树只能留下所有引用者的并集，于是工具窗口为了设置控件里
 * 那十几个图标，也得下载整套。创作台那一遍让 iconLookup 拿一份自己的 `lib/icons`
 * 和 react-icons 副本，工具窗口那份只剩它自己用到的。
 */
export function studioIconCatalogPlugin() {
  const COPY = '?icon-catalog'
  return {
    name: 'myriad:studio-icon-catalog',
    apply: () => isStudioPass(),
    enforce: 'pre',
    async resolveId(source, importer, options) {
      if (!importer) return null
      const fromCatalog = importer.endsWith('/src/lib/iconLookup.ts') && source === './icons'
      const fromCopy = importer.endsWith(`/src/lib/icons.tsx${COPY}`) && source.startsWith('react-icons/')
      if (!fromCatalog && !fromCopy) return null
      const resolved = await this.resolve(source, importer.replace(COPY, ''), { ...options, skipSelf: true })
      return resolved && { ...resolved, id: resolved.id + COPY }
    },
  }
}
