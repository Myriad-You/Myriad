/**
 * 嵌入页（`embed.html`：伴读、评论、友链申请、创作台）是构建里的第二个环境。和主站放进同一个环境，两个入口
 * 共用的初始模块会被 app-shell 合成一块，每个嵌入页就又背上了主站外壳；按入口拆开
 * （entriesAware）又会把主站首页拆成几十个预加载小块。各自一个环境，模块图和分块互不相干，
 * 产物写进同一个目录。开发时不需要它：开发服务器把嵌入页的地址交给 `embed.html`，照常
 * 由 client 环境处理。
 */

export const EMBED_ENVIRONMENT = 'embed'

/** @param {{ name: string }} environment */
export function isEmbedEnvironment(environment) {
  return environment.name === EMBED_ENVIRONMENT
}

/** 先主站、再嵌入页；第二个环境不清空目录、不再拷 public/。 */
export async function buildBothEnvironments(builder) {
  await builder.build(builder.environments.client)
  await builder.build(builder.environments[EMBED_ENVIRONMENT])
}

/**
 * 笔记编辑器里按名字找图标的 `lib/iconLookup.ts` 引用了近百个品牌图标。react-icons
 * 每套图标是一个模块，摇树只能留下所有引用者的并集，于是工具窗口为了设置控件里
 * 那十几个图标，也得下载整套。嵌入页环境里让 iconLookup 拿一份自己的 `lib/icons`
 * 和 react-icons 副本，工具窗口那份只剩它自己用到的。
 */
export function embedIconCatalogPlugin() {
  const COPY = '?icon-catalog'
  return {
    name: 'myriad:embed-icon-catalog',
    applyToEnvironment: isEmbedEnvironment,
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
