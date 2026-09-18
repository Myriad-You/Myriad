export interface SettingGuideEntry {
  what: string
  chain?: string
  frontend?: string
  notes?: string
}

export interface SettingGuidesCatalog {
  ui: {
    siteUrl: SettingGuideEntry
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
    siteSeoReviewCadence: SettingGuideEntry
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
    visitorStats: SettingGuideEntry
    /** this browser only */
    analyticsOptOut: SettingGuideEntry
    pageAnalytics: SettingGuideEntry
    eventAnalytics: SettingGuideEntry
    referrerAnalytics: SettingGuideEntry
    aiUsage: SettingGuideEntry
    thirdPartyAnalytics: SettingGuideEntry
    gaMeasurementId: SettingGuideEntry
    umamiWebsiteId: SettingGuideEntry
    umamiScriptUrl: SettingGuideEntry
    connected: SettingGuideEntry
    autoRefresh: SettingGuideEntry
    platformCard: SettingGuideEntry
    platformFields: SettingGuideEntry
    dataPreview: SettingGuideEntry
    dataManagement: SettingGuideEntry
    dataRefresh: SettingGuideEntry
    dataReprocess: SettingGuideEntry
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
    imageModel: SettingGuideEntry
    speechStt: SettingGuideEntry
    speechTts: SettingGuideEntry
    speechVoice: SettingGuideEntry
  }
  agent: {
    agentPersona: SettingGuideEntry
    agentPersonaSpeech: SettingGuideEntry
    channels: SettingGuideEntry
    qqBot: SettingGuideEntry
    telegramBot: SettingGuideEntry
    discordBot: SettingGuideEntry
    feishuBot: SettingGuideEntry
    heartbeat: SettingGuideEntry
    skills: SettingGuideEntry
    memory: SettingGuideEntry
  }
  /** keys stay on backend; settings nav / URL section is lab */
  tripo: {
    connection: SettingGuideEntry
    enabled: SettingGuideEntry
    apiKey: SettingGuideEntry
    baseUrl: SettingGuideEntry
    webBudget: SettingGuideEntry
    model: SettingGuideEntry
    faceLimit: SettingGuideEntry
    maxDownload: SettingGuideEntry
    taskControl: SettingGuideEntry
    pollInterval: SettingGuideEntry
    taskTimeout: SettingGuideEntry
  }
  oauth: {
    section: SettingGuideEntry
    provider: SettingGuideEntry
    callback: SettingGuideEntry
    clientId: SettingGuideEntry
    clientSecret: SettingGuideEntry
    discovery: SettingGuideEntry
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
    allowLocalRegister: SettingGuideEntry
    privateTappInstall: SettingGuideEntry
  }
  advanced: {
    memorySaver: SettingGuideEntry
    memorySaverEnable: SettingGuideEntry
    preciseLocation: SettingGuideEntry
    preciseLocationEnable: SettingGuideEntry
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
    frontendCache: SettingGuideEntry
    forceRefreshCache: SettingGuideEntry
    runtimeDiagnostics: SettingGuideEntry
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
    snapshotLimit: SettingGuideEntry
    checkInterval: SettingGuideEntry
    autoInstall: SettingGuideEntry
    advanced: SettingGuideEntry
    transport: SettingGuideEntry
    token: SettingGuideEntry
  }
  about: {
    section: SettingGuideEntry
  }
  /** not a /config section; search may skip */
  tapp: {
    detail: SettingGuideEntry
    overview: SettingGuideEntry
    appSettings: SettingGuideEntry
    appVisibility: SettingGuideEntry
    permissions: SettingGuideEntry
    permPrivileged: SettingGuideEntry
    permElevated: SettingGuideEntry
    permBasic: SettingGuideEntry
  }
}

export interface GuideSectionLabels {
  what: string
  chain: string
  frontend: string
  notes: string
}
