/**
 * 设置选项详细指南（弹窗正文结构）
 */

export interface SettingGuideEntry {
  /** 作用：这个选项控制什么 */
  what: string
  /** 关联链路：配置 → 存储/API → 下游模块（尽量写全） */
  chain?: string
  /** 前端落点：具体页面 / 组件 / 入口 */
  frontend?: string
  /** 注意：依赖其它设置、保存方式、危险操作 */
  notes?: string
}

/** 分区指南目录 */
export interface SettingGuidesCatalog {
  ui: {
    siteUrl: SettingGuideEntry
    /** 基础设置：网站名片 + PWA（合并组） */
    siteIdentity: SettingGuideEntry
    siteTitle: SettingGuideEntry
    siteDescription: SettingGuideEntry
    siteFavicon: SettingGuideEntry
    siteSeo: SettingGuideEntry
    siteKeywords: SettingGuideEntry
    siteOgImage: SettingGuideEntry
    googleSiteVerification: SettingGuideEntry
    siteVisibilityPolicy: SettingGuideEntry
    siteAiIntro: SettingGuideEntry
    siteAiGenerate: SettingGuideEntry
    pwaEnabled: SettingGuideEntry
    siteFooter: SettingGuideEntry
    siteIcp: SettingGuideEntry
    siteGongan: SettingGuideEntry
    cloudSponsors: SettingGuideEntry
    siteFooterCustom: SettingGuideEntry
    backgroundAndTheme: SettingGuideEntry
    wallpaper: SettingGuideEntry
    wallpaperBlur: SettingGuideEntry
    evocative: SettingGuideEntry
    evocativeEffects: SettingGuideEntry
    evocativeFps: SettingGuideEntry
    evocativeRippleQuality: SettingGuideEntry
  }
  modules: {
    visibility: SettingGuideEntry
    visibilityItem: SettingGuideEntry
    library: SettingGuideEntry
    /** 资料库卡片排布：列表 | 无限画布 */
    libraryLayout: SettingGuideEntry
    libraryType: SettingGuideEntry
    report: SettingGuideEntry
    reportExpiry: SettingGuideEntry
    reportAutoRegen: SettingGuideEntry
    reportExpiryDays: SettingGuideEntry
    music: SettingGuideEntry
    musicPlatform: SettingGuideEntry
    musicPlaylist: SettingGuideEntry
    musicCache: SettingGuideEntry
    hitokoto: SettingGuideEntry
    hitokotoSource: SettingGuideEntry
    hitokotoCustomUrl: SettingGuideEntry
    hitokotoTextField: SettingGuideEntry
    hitokotoAuthorField: SettingGuideEntry
    island: SettingGuideEntry
    islandContent: SettingGuideEntry
  }
  platforms: {
    list: SettingGuideEntry
    /** 数据及统计：访客统计子分类 */
    visitorStats: SettingGuideEntry
    /** 本机退出采集（只影响这台浏览器） */
    analyticsOptOut: SettingGuideEntry
    /** 数据及统计：页面访问分析（访客统计组内分区） */
    pageAnalytics: SettingGuideEntry
    /** 数据及统计：事件埋点（访客统计组内分区） */
    eventAnalytics: SettingGuideEntry
    /** 数据及统计：来源站点（访客统计组内分区） */
    referrerAnalytics: SettingGuideEntry
    /** 数据及统计：AI 使用统计 */
    aiUsage: SettingGuideEntry
    /** 数据及统计：第三方统计（GA / Umami） */
    thirdPartyAnalytics: SettingGuideEntry
    gaMeasurementId: SettingGuideEntry
    umamiWebsiteId: SettingGuideEntry
    umamiScriptUrl: SettingGuideEntry
    /** 数据及统计：接入平台子分类 */
    connected: SettingGuideEntry
    autoRefresh: SettingGuideEntry
    platformCard: SettingGuideEntry
    platformFields: SettingGuideEntry
    /** 平台二级页：当前数据快照（一次加载） */
    dataPreview: SettingGuideEntry
    /** 平台二级页：数据管理整组 */
    dataManagement: SettingGuideEntry
    /** 拉取/刷新原始数据 */
    dataRefresh: SettingGuideEntry
    /** 重新处理已有原始数据 */
    dataReprocess: SettingGuideEntry
    /** 清空本站缓存 */
    dataClearCache: SettingGuideEntry
  }
  notifications: {
    master: SettingGuideEntry
    island: SettingGuideEntry
    toast: SettingGuideEntry
    browser: SettingGuideEntry
    source: SettingGuideEntry
    locations: SettingGuideEntry
    events: SettingGuideEntry
  }
  ai: {
    llm: SettingGuideEntry
    standard: SettingGuideEntry
    lite: SettingGuideEntry
    liteEnable: SettingGuideEntry
    agentPersona: SettingGuideEntry
    agentPersonaSpeech: SettingGuideEntry
    qqBot: SettingGuideEntry
    telegramBot: SettingGuideEntry
    pro: SettingGuideEntry
    proEnable: SettingGuideEntry
    image: SettingGuideEntry
    speech: SettingGuideEntry
    vendors: SettingGuideEntry
    webSearch: SettingGuideEntry
    provider: SettingGuideEntry
    apiKey: SettingGuideEntry
    baseUrl: SettingGuideEntry
    model: SettingGuideEntry
    /** Agent 定时任务 */
    heartbeat: SettingGuideEntry
    /** Agent 技能列表 */
    skills: SettingGuideEntry
    /** Agent 记忆列表 */
    memory: SettingGuideEntry
    /** 图片生成模型名 */
    imageModel: SettingGuideEntry
    /** 语音识别模型 */
    speechStt: SettingGuideEntry
    /** 语音合成模型 */
    speechTts: SettingGuideEntry
    /** 语音合成音色 */
    speechVoice: SettingGuideEntry
  }
  /**
   * 3D / Tripo（独立于 AI 图片服务；密钥仅存后端）
   * ConfigForm 一级 section id = tripo
   */
  tripo: {
    /** 连接组：启用 / API Key / Base URL */
    connection: SettingGuideEntry
    enabled: SettingGuideEntry
    apiKey: SettingGuideEntry
    baseUrl: SettingGuideEntry
    /** Web 模型预算：默认低模、面数、落盘上限 */
    webBudget: SettingGuideEntry
    model: SettingGuideEntry
    faceLimit: SettingGuideEntry
    maxDownload: SettingGuideEntry
    /** 任务控制：轮询间隔 / 超时 */
    taskControl: SettingGuideEntry
    pollInterval: SettingGuideEntry
    taskTimeout: SettingGuideEntry
  }
  oauth: {
    section: SettingGuideEntry
    provider: SettingGuideEntry
    /** 登录回调地址 */
    callback: SettingGuideEntry
    clientId: SettingGuideEntry
    clientSecret: SettingGuideEntry
    discovery: SettingGuideEntry
    /** 标识 / 显示名 / 范围 / 图标 */
    advanced: SettingGuideEntry
  }
  permissions: {
    agentPreset: SettingGuideEntry
    agentPresetUser: SettingGuideEntry
    agentPresetGuest: SettingGuideEntry
    fineTune: SettingGuideEntry
    userElevated: SettingGuideEntry
    guestElevated: SettingGuideEntry
    aiQuota: SettingGuideEntry
    userQuota: SettingGuideEntry
    guestQuota: SettingGuideEntry
  }
  users: {
    section: SettingGuideEntry
    create: SettingGuideEntry
    list: SettingGuideEntry
    /** 公开本地用户名密码注册（/register） */
    allowLocalRegister: SettingGuideEntry
    /** 普通用户个人（临时）Tapp 安装清理 */
    privateTappInstall: SettingGuideEntry
  }
  advanced: {
    memorySaver: SettingGuideEntry
    memorySaverEnable: SettingGuideEntry
    network: SettingGuideEntry
    proxyEnable: SettingGuideEntry
    proxyUrl: SettingGuideEntry
    proxyBypass: SettingGuideEntry
    geminiBaseUrl: SettingGuideEntry
    githubApiBaseUrl: SettingGuideEntry
    backup: SettingGuideEntry
    exportConfig: SettingGuideEntry
    importConfig: SettingGuideEntry
    resetConfig: SettingGuideEntry
    /** 前端缓存强制刷新 */
    frontendCache: SettingGuideEntry
    forceRefreshCache: SettingGuideEntry
    /** 运行与诊断：DB / 存储 / 出口 / 版本等只读检查 */
    runtimeDiagnostics: SettingGuideEntry
    /** MCP 工具服务器状态与热重载 */
    mcp: SettingGuideEntry
    mcpId: SettingGuideEntry
    mcpCommand: SettingGuideEntry
    mcpArgs: SettingGuideEntry
    mcpEnv: SettingGuideEntry
    mcpMaxRestart: SettingGuideEntry
    mcpEnabled: SettingGuideEntry
    mcpAutoRestart: SettingGuideEntry
    mcpTrustAnnotations: SettingGuideEntry
  }
  federation: {
    keys: SettingGuideEntry
    /** 轮换签名密钥 */
    rotateKeys: SettingGuideEntry
    policy: SettingGuideEntry
    minTrust: SettingGuideEntry
    allowlist: SettingGuideEntry
    autoDiscover: SettingGuideEntry
    knownInstances: SettingGuideEntry
    contentFilters: SettingGuideEntry
    deliveryQueue: SettingGuideEntry
    advanced: SettingGuideEntry
    rateMax: SettingGuideEntry
    rateWindow: SettingGuideEntry
    rateTrusted: SettingGuideEntry
  }
  updater: {
    channel: SettingGuideEntry
    maintenance: SettingGuideEntry
    rescue: SettingGuideEntry
    forceExit: SettingGuideEntry
    infra: SettingGuideEntry
    target: SettingGuideEntry
    snapshot: SettingGuideEntry
    /** 备份数量上限（备份与回退区） */
    snapshotLimit: SettingGuideEntry
    /** 检查频率 / 自动安装（状态区） */
    checkInterval: SettingGuideEntry
    autoInstall: SettingGuideEntry
    /** 连接方式、口令、诊断（折叠高级面板） */
    advanced: SettingGuideEntry
    transport: SettingGuideEntry
    token: SettingGuideEntry
  }
  about: {
    section: SettingGuideEntry
  }
  /**
   * Tapp 应用详情页（非 /config 分区；搜索索引可跳过）。
   * 仅覆盖页面通用文案，不写各应用 manifest 自定义设置项。
   */
  tapp: {
    /** 详情页整体 */
    detail: SettingGuideEntry
    /** 顶部应用信息与操作 */
    overview: SettingGuideEntry
    /** 应用设置组 */
    appSettings: SettingGuideEntry
    /** 公开安装可见性 */
    appVisibility: SettingGuideEntry
    /** 已授权权限组 */
    permissions: SettingGuideEntry
    /** 特权权限子组 */
    permPrivileged: SettingGuideEntry
    /** 提升权限子组 */
    permElevated: SettingGuideEntry
    /** 基础权限子组 */
    permBasic: SettingGuideEntry
  }
}

export interface GuideSectionLabels {
  what: string
  chain: string
  frontend: string
  notes: string
}
