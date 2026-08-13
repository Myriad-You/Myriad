/**
 * Tapp 各权限项的结构化指南（详情页权限卡「选项指南」）。
 * 与通用 tapp.* 分区指南分开维护，避免 SettingGuidesCatalog 膨胀。
 */

import type { Locale } from '../../../i18n'
import type { TappPermission } from '../../../tapp/types'
import type { SettingGuideEntry } from './types'

export type TappPermissionGuides = Record<TappPermission, SettingGuideEntry>

/** 指南锚点路径，如 tapp.perm.widget:register */
export function tappPermissionGuidePath(permission: TappPermission): string {
  return `tapp.perm.${permission}`
}

const zh: TappPermissionGuides = {
  'widget:register': {
    what: '允许把自定义小组件注册到站点主页网格。',
    chain:
      '① 应用声明并获批本权限。\n② 调用小组件注册 API 后，主页出现对应卡片。\n③ 仅管理员级能力，影响全站访客看到的主页。',
    frontend: '主页 Widget 网格；应用内注册/注销小组件的入口。',
    notes: '恶意或劣质小组件会直接出现在主页，只装可信应用。',
  },
  'platform:read': {
    what: '允许读取已接入的平台数据（游戏、视频、音乐等已有条目）。',
    chain:
      '① 应用通过平台读 API 取数。\n② 数据来自本站已配置并同步的平台。\n③ 只读，不改平台配置本身。',
    frontend: '应用内展示的平台统计、列表、进度等。',
    notes: '读到的是本站已有数据；未配置的平台通常为空。',
  },
  'platform:write': {
    what: '允许向平台数据写入新条目或更新内容。',
    chain:
      '① 应用调用写 API。\n② 写入进入本站平台数据存储。\n③ 报告页、资料库等依赖这些数据的地方可能一起变。',
    frontend: '应用提供的录入/同步功能；之后主站相关展示。',
    notes: '写权限风险高，可能污染统计与展示，请确认应用可信。',
  },
  'platform:register': {
    what: '允许注册自定义数据平台类型，扩展本站可接入的平台种类。',
    chain:
      '① 应用注册平台定义。\n② 站点平台列表出现新类型。\n③ 后续可按该类型配置与拉数。',
    frontend: '设置「数据及统计 / 接入平台」及相关配置。',
    notes: '属于站点级扩展，勿装来源不明的应用。',
  },
  'analytics:read': {
    what: '允许读取站点访问统计的汇总数据（今日/区间访问、热门页、来源等）。',
    chain:
      '① 应用声明并获批本权限。\n② 通过 Tapp.analytics 读取聚合摘要。\n③ 仅汇总，不含访客指纹或原始日志。',
    frontend: '应用内统计卡片、访客概览等；设置「站点分析」为数据源。',
    notes: '只读聚合；不开放导入导出或 AI 用量明细。',
  },
  'ai:generate': {
    what: '允许调用 AI 生成内容（文案、摘要等，视站点模型配置而定）。',
    chain:
      '① 应用发起生成请求。\n② 经本站 AI 网关与配额校验。\n③ 结果回传应用展示。',
    frontend: '应用内的生成按钮、结果区；设置里的 AI 与配额。',
    notes: '会消耗 AI 配额；模型与密钥在站点 AI 配置中。',
  },
  'ai:analyze': {
    what: '允许调用 AI 做数据分析或结构化解读。',
    chain:
      '① 应用提交待分析内容。\n② 走分析类 AI 调用与配额。\n③ 返回分析结论供界面展示。',
    frontend: '应用内的分析/洞察类功能。',
    notes: '同样计费/计配额；大数据量可能更慢或失败。',
  },
  'ai:chat': {
    what: '允许以对话方式调用 AI（多轮聊天能力）。',
    chain:
      '① 应用打开对话会话。\n② 消息经 AI 聊天通道。\n③ 历史可能仅存在于该应用侧。',
    frontend: '应用内聊天界面；全局 Arael 面板是另一套能力。',
    notes: '对话轮次会累计消耗配额。',
  },
  'ai:image': {
    what: '允许调用 AI 生成图片。',
    chain:
      '① 应用提交出图请求。\n② 使用站点配置的图像模型。\n③ 返回图片 URL 或数据供展示。',
    frontend: '应用内的出图/配图功能。',
    notes: '图像模型通常更贵或更慢；需站点已配置 image 能力。',
  },
  'report:read': {
    what: '允许读取本站已生成的报告数据。',
    chain:
      '① 应用请求报告内容。\n② 读取报告存储中的结果。\n③ 在应用内渲染。',
    frontend: '报告页已有内容；应用内的报告视图。',
    notes: '只能读已有报告，不能因此创建新报告。',
  },
  'report:write': {
    what: '允许创建、更新或删除报告。',
    chain:
      '① 应用写入报告 API。\n② 报告列表与内容变更。\n③ 报告页展示随之变化。',
    frontend: '报告模块列表与详情。',
    notes: '可删除用户可见报告，属高敏感写权限。',
  },
  'storage:read': {
    what: '允许使用该应用的本地/服务端键值存储（应用私有数据）。',
    chain:
      '① 应用读写 storage API。\n② 数据按应用隔离保存。\n③ 卸载时可选是否清除。',
    frontend: '应用自身的偏好、草稿、缓存状态。',
    notes: '一般不共享给其他应用；别存密钥到不可信应用。',
  },
  'storage:write': {
    what: '允许写入该应用的私有键值存储。',
    chain: '① 应用调用写入 API。\n② 数据按应用隔离保存。\n③ 写入内容会影响应用后续状态。',
    frontend: '应用偏好、草稿和持久化状态。',
    notes: '写入权限默认关闭，仅向可信应用下放。',
  },
  'ui:notification': {
    what: '允许向用户发送系统通知（站内通知等）。',
    chain:
      '① 应用调用通知 API。\n② 进入通知中心 / 岛屿等展示通道。\n③ 用户可在通知设置里调整接收方式。',
    frontend: '导航通知、Toast、浏览器通知（若用户允许）。',
    notes: '请勿滥用刷屏；用户可关闭部分通道。',
  },
  'ui:fullscreen': {
    what: '允许请求浏览器全屏显示。',
    chain:
      '① 应用调用全屏 API。\n② 浏览器可能弹出确认。\n③ 用户可随时退出全屏。',
    frontend: '应用运行页的全屏体验。',
    notes: '多数浏览器要求用户手势后才能进入全屏。',
  },
  'ui:theme': {
    what: '允许读取当前站点主题（明暗、主色等），以便界面适配。',
    chain:
      '① 应用查询主题 API。\n② 拿到当前主题令牌。\n③ 用于自身样式，不改站点主题。',
    frontend: '应用 UI 颜色与站点一致。',
    notes: '只读；改站点主题在设置「外观」里。',
  },
  'ui:confirm': {
    what: '允许弹出系统确认对话框（危险操作二次确认等）。',
    chain:
      '① 应用请求确认框。\n② 用户点确定/取消。\n③ 结果返回应用再继续逻辑。',
    frontend: '运行中的模态确认。',
    notes: '频繁弹窗会骚扰用户，应用应克制使用。',
  },
  'ui:openUrl': {
    what: '允许由宿主打开 Manifest openUrls 中声明的外链（未声明地址一律拒绝）。',
    chain:
      '① 应用声明 openUrls 与本权限并获安装授权。\n② 调用 Tapp.ui.openUrl({ id })（可带 path/query，受 match 约束）。\n③ 宿主校验 allowlist 后新标签打开，无二次确认框。',
    frontend: '应用内按钮/卡片点击跳转；详情页可见 openUrls 权限说明。',
    notes:
      '只能打开安装时声明的链接；沙箱内 window.open 仍禁用。安装时请核对声明域名。',
  },
  'network:fetch': {
    what: '允许声明式 HTTP 请求，以及加载远端图片/音视频等资源。',
    chain:
      '① 应用声明接口或加载外链资源。\n② 经宿主代理/校验后访问网络。\n③ 响应回到沙箱应用。',
    frontend: '应用拉取的外部数据、封面、媒体。',
    notes: '可能受代理、CORS、站点网络安全策略限制；有外联风险。',
  },
  'media:control': {
    what: '允许控制媒体播放（播放、暂停、切歌等）。',
    chain:
      '① 应用调用媒体控制 API。\n② 作用于站点媒体/播放器状态。\n③ 控制面板音乐等可能同步变化。',
    frontend: '全局音乐播放器、应用内播放控件。',
    notes: '可能打断用户正在听的内容。',
  },
  'media:read': {
    what: '允许读取当前媒体播放状态（曲目、进度、是否播放等）。',
    chain:
      '① 应用查询媒体状态。\n② 只读当前播放会话信息。\n③ 用于歌词、可视化等。',
    frontend: '应用内的「正在播放」展示。',
    notes: '不包含改播放列表的能力（那是 control）。',
  },
  'media:audio': {
    what: '允许在沙箱内播放包内、blob 或 data 音频。',
    chain:
      '① 应用触发音频播放。\n② 仅限允许来源的音频。\n③ 不自动获得任意外链播控。',
    frontend: '应用内音效、语音片段播放。',
    notes: '与 media:control 不同，侧重沙箱内音频源播放。',
  },
  'component:theme': {
    what: '允许注册自定义主题样式组件，扩展站点主题选项。',
    chain:
      '① 应用注册主题。\n② 主题列表出现新项。\n③ 用户选用后全局外观变化。',
    frontend: '设置外观/主题相关入口。',
    notes: '影响全站观感，只装可信主题应用。',
  },
  'component:agent': {
    what: '允许向 Arael Agent 注册能力（工具/技能）。',
    chain:
      '① 应用注册 Agent 能力。\n② 助手可调用这些能力。\n③ 对话中可能触发对应动作。',
    frontend: 'Arael 面板与 Agent 相关交互。',
    notes: '等于把能力挂到全站助手，权限极高。',
  },
  'shortcut:register': {
    what: '允许注册键盘快捷键。',
    chain:
      '① 应用声明快捷键。\n② 在页面聚焦时拦截按键。\n③ 触发应用定义的动作。',
    frontend: '浏览站点时的全局/页面快捷键。',
    notes: '可能与浏览器或其它应用快捷键冲突。',
  },
  'event:publish': {
    what: '允许向系统事件总线发布事件，供其它模块订阅。',
    chain:
      '① 应用发布事件。\n② 订阅方收到并处理。\n③ 可实现跨组件联动。',
    frontend: '依赖事件联动的界面刷新、通知等。',
    notes: '乱发事件会造成干扰，发布方应使用约定事件名。',
  },
  'event:subscribe': {
    what: '允许订阅系统事件，在事件发生时收到回调。',
    chain:
      '① 应用订阅某类事件。\n② 有发布时推送到应用。\n③ 应用更新自身状态。',
    frontend: '应用随系统状态自动刷新的部分。',
    notes: '只订阅不发布；风险低于 publish。',
  },
  'scheduler:register': {
    what: '允许注册和管理定时任务（到点执行后台逻辑）。',
    chain:
      '① 应用注册调度任务。\n② 宿主按计划触发。\n③ 任务可读写应用允许的其它 API。',
    frontend: '应用后台定时同步、提醒等。',
    notes: '任务过于频繁会耗资源；卸载后应清理任务。',
  },
  'speech:tts': {
    what: '允许使用文本转语音（朗读文字）。',
    chain:
      '① 应用提交文本。\n② 经站点语音/TTS 配置合成。\n③ 播放或返回音频。',
    frontend: '应用内「朗读」类功能。',
    notes: '依赖站点是否配置 TTS；可能消耗配额。',
  },
  'speech:asr': {
    what: '允许使用语音识别（把语音转成文字）。',
    chain:
      '① 应用采集或上传音频。\n② 经 ASR 服务识别。\n③ 返回文本给应用。',
    frontend: '应用内语音输入。',
    notes: '通常需要麦克风权限；注意隐私。',
  },
  'tappList:read': {
    what: '允许读取已安装的 Tapp 列表及基本信息。',
    chain:
      '① 应用查询列表 API。\n② 得到安装清单摘要。\n③ 用于展示或联动其它应用。',
    frontend: '应用内的「已安装应用」类界面。',
    notes: '可能暴露你装了哪些应用，注意隐私。',
  },
  'tappList:manage': {
    what: '允许安装、更新、启动/停止或卸载 Tapp。',
    chain:
      '① 应用调用管理 API。\n② 改变本站安装状态。\n③ 列表与运行态同步变化。',
    frontend: 'Tapp 列表、详情页的安装与运行控制。',
    notes: '可增删任意应用，属最高危能力之一。',
  },
  'brew:read': {
    what: '允许读取 Brew 订阅源、文章内容与评论/回复。',
    chain:
      '① 应用请求 Brew 列表/正文/评论。\n② 读取本站已订阅数据。\n③ 在应用内展示阅读内容。',
    frontend: 'Brew 模块与应用内阅读视图。',
    notes: '只读订阅数据，不能改已读状态、收藏或写评论。',
  },
  'brew:readStatus': {
    what: '允许标记当前用户的已读/未读/全部已读状态。',
    chain:
      '① 应用更新条目阅读状态。\n② 只改当前用户自己的状态。\n③ Brew 列表筛选与角标可能变化。',
    frontend: 'Brew 已读状态。',
    notes: '只影响你自己的阅读状态，不影响共享内容。',
  },
  'brew:favorite': {
    what: '允许收藏/取消收藏当前用户的文章（星标）。',
    chain:
      '① 应用更新条目星标。\n② 只改当前用户自己的收藏。\n③ Brew 星标筛选随之变化。',
    frontend: 'Brew 收藏/星标。',
    notes: '只影响你自己的收藏，不影响共享内容。',
  },
  'brew:commentWrite': {
    what: '允许创建、编辑和删除 Brew 评论与回复。',
    chain:
      '① 应用提交评论。\n② 评论关联到文章。\n③ 其它读者可能看到（视站点策略）。',
    frontend: 'Brew 文章评论区。',
    notes: '涉及公开表达时注意内容规范；默认仅管理员可下放。',
  },
  'brew:manage': {
    what: '允许管理 Brew 订阅源与高级设置（添加源、发现源等）。',
    chain:
      '① 应用调用源管理 API。\n② 订阅源集合变更。\n③ 抓取与列表随之更新。',
    frontend: 'Brew 源管理、OPML、发现源。',
    notes: '可增删订阅源，影响全站 Brew 内容池。',
  },
  'federation:read': {
    what: '允许读取联邦房间、频道、成员等数据。',
    chain:
      '① 应用查询联邦 API。\n② 返回互联实例可见的数据。\n③ 用于展示联邦状态。',
    frontend: '联邦相关页面与应用内联邦视图。',
    notes: '能看到的范围仍受信任策略限制。',
  },
  'federation:write': {
    what: '允许创建或更新联邦资源（房间、频道等）。',
    chain:
      '① 应用写入联邦 API。\n② 资源在本站与可能的对端变更。\n③ 成员可见结构变化。',
    frontend: '联邦管理与创建流程。',
    notes: '写操作可能同步到外部实例，谨慎授权。',
  },
  'federation:message': {
    what: '允许发送和接收联邦消息。',
    chain:
      '① 应用收发消息。\n② 经联邦投递队列。\n③ 对端实例可能收到内容。',
    frontend: '联邦消息/会话界面。',
    notes: '消息可能离开本站，注意隐私与滥用。',
  },
  'federation:trust': {
    what: '允许管理联邦信任关系（谁可信、谁拉黑等）。',
    chain:
      '① 应用调整信任策略。\n② 影响入站/出站是否接受。\n③ 过滤与投递行为改变。',
    frontend: '设置「联邦」中的信任与策略相关项。',
    notes: '错误配置可能导致无法互联或误信恶意实例。',
  },
  'federation:files': {
    what: '允许使用联邦文件传输能力（收发文件类资源）。',
    chain:
      '① 应用发起文件相关 API。\n② 经联邦文件通道。\n③ 文件到达对端或本地下载。',
    frontend: '联邦场景下的附件/文件功能。',
    notes: '文件体积与类型可能受限；注意恶意文件风险。',
  },
}

const en: TappPermissionGuides = {
  'widget:register': {
    what: 'Allows registering custom widgets on the home grid.',
    chain:
      '1) App is granted this permission.\n2) Widget register API places cards on the home grid.\n3) Admin-level; all visitors can see those widgets.',
    frontend: 'Home widget grid; in-app register/unregister controls.',
    notes: 'Only install trusted apps — widgets appear site-wide.',
  },
  'platform:read': {
    what: 'Allows reading existing platform data (games, video, music entries, etc.).',
    chain:
      '1) App calls platform read APIs.\n2) Data comes from configured, synced platforms.\n3) Read-only; does not change platform credentials.',
    frontend: 'In-app stats, lists, progress views.',
    notes: 'Unconfigured platforms usually return empty data.',
  },
  'platform:write': {
    what: 'Allows writing or updating platform data entries.',
    chain:
      '1) App calls write APIs.\n2) Data is stored in site platform storage.\n3) Reports / library views that depend on it may change.',
    frontend: 'In-app import/sync; later site displays.',
    notes: 'High impact — bad writes can pollute stats.',
  },
  'platform:register': {
    what: 'Allows registering a custom platform type on the site.',
    chain:
      '1) App registers a platform definition.\n2) It appears among connectable platforms.\n3) Admins can configure and sync that type.',
    frontend: 'Config → Platforms.',
    notes: 'Site-level extension; trust the app source.',
  },
  'analytics:read': {
    what: 'Allows reading aggregated site analytics (visits, top pages, referrers, etc.).',
    chain:
      '1) App is granted this permission.\n2) Calls Tapp.analytics for summary cards.\n3) Aggregate-only — no visitor hashes or raw logs.',
    frontend: 'In-app analytics cards; site analytics as the data source.',
    notes: 'Read-only aggregates; no backup import/export or AI usage ledgers.',
  },
  'ai:generate': {
    what: 'Allows AI content generation (text/summaries per site models).',
    chain:
      '1) App requests generation.\n2) Site AI gateway + quota checks run.\n3) Result returns to the app.',
    frontend: 'In-app generate actions; AI settings & quotas.',
    notes: 'Consumes AI quota; keys/models live in site AI config.',
  },
  'ai:analyze': {
    what: 'Allows AI analysis of data or structured interpretation.',
    chain:
      '1) App submits content to analyze.\n2) Analysis calls consume quota.\n3) Insights render in the app.',
    frontend: 'In-app analysis / insight features.',
    notes: 'Large payloads may be slow or fail.',
  },
  'ai:chat': {
    what: 'Allows multi-turn AI chat through the site gateway.',
    chain:
      '1) App opens a chat session.\n2) Messages go through AI chat.\n3) History may stay only in the app.',
    frontend: 'In-app chat UI (separate from the global Arael panel).',
    notes: 'Each turn can consume quota.',
  },
  'ai:image': {
    what: 'Allows AI image generation.',
    chain:
      '1) App requests an image.\n2) Uses the site image model config.\n3) Returns image URL/data.',
    frontend: 'In-app image generation.',
    notes: 'Often slower/costlier; image capability must be configured.',
  },
  'report:read': {
    what: 'Allows reading existing generated reports.',
    chain:
      '1) App requests report content.\n2) Reads from report storage.\n3) Renders inside the app.',
    frontend: 'Reports module; in-app report views.',
    notes: 'Does not create reports by itself.',
  },
  'report:write': {
    what: 'Allows creating, updating, or deleting reports.',
    chain:
      '1) App writes via report APIs.\n2) Report list/content changes.\n3) Reports page updates.',
    frontend: 'Reports list and detail.',
    notes: 'Can delete user-visible reports — sensitive.',
  },
  'storage:read': {
    what: 'Allows private key-value storage for this app.',
    chain:
      '1) App uses storage APIs.\n2) Data is isolated per app.\n3) Uninstall may clear it (user choice).',
    frontend: 'App preferences, drafts, local state.',
    notes: 'Do not store secrets in untrusted apps.',
  },
  'storage:write': {
    what: 'Allows writing to this app private key-value storage.',
    chain: '1) App calls write APIs.\n2) Data is isolated per app.\n3) Writes affect later app state.',
    frontend: 'App preferences, drafts, and persistent state.',
    notes: 'Write access is disabled by default; delegate only to trusted apps.',
  },
  'ui:notification': {
    what: 'Allows sending system notifications to the user.',
    chain:
      '1) App calls notification APIs.\n2) Delivery via center / island / toast channels.\n3) User preferences still apply.',
    frontend: 'Notification center, toasts, browser push if allowed.',
    notes: 'Avoid spam; users can mute channels.',
  },
  'ui:fullscreen': {
    what: 'Allows requesting browser fullscreen.',
    chain:
      '1) App calls fullscreen API.\n2) Browser may prompt.\n3) User can exit anytime.',
    frontend: 'Fullscreen run experience.',
    notes: 'Usually requires a user gesture.',
  },
  'ui:theme': {
    what: 'Allows reading the current site theme for UI matching.',
    chain:
      '1) App queries theme APIs.\n2) Receives theme tokens.\n3) Styles itself; does not change site theme.',
    frontend: 'App chrome matching light/dark and accent.',
    notes: 'Read-only; change theme in UI settings.',
  },
  'ui:confirm': {
    what: 'Allows showing system confirm dialogs.',
    chain:
      '1) App requests a confirm box.\n2) User accepts or cancels.\n3) Result returns to the app.',
    frontend: 'Modal confirms while the app runs.',
    notes: 'Excessive prompts annoy users.',
  },
  'ui:openUrl': {
    what: 'Allows the host to open links declared in Manifest openUrls (all other URLs are rejected).',
    chain:
      '1) App declares openUrls + this permission and is approved at install.\n2) Calls Tapp.ui.openUrl({ id }) (optional path/query within match rules).\n3) Host checks the allowlist and opens a new tab — no extra confirm dialog.',
    frontend: 'In-app buttons/cards that open external links; permission copy on the detail page.',
    notes:
      'Only declared links work; sandbox window.open stays disabled. Review declared hosts at install time.',
  },
  'network:fetch': {
    what: 'Allows declarative HTTP and loading remote media/images.',
    chain:
      '1) App declares endpoints or remote assets.\n2) Host proxies/validates network access.\n3) Response returns to the sandbox.',
    frontend: 'External data, covers, media in the app.',
    notes: 'Subject to proxy/CORS/security policy; outbound risk.',
  },
  'media:control': {
    what: 'Allows controlling playback (play, pause, skip, etc.).',
    chain:
      '1) App calls media control APIs.\n2) Affects site player state.\n3) Control-panel music may update.',
    frontend: 'Global music player; in-app transport controls.',
    notes: 'May interrupt what the user is listening to.',
  },
  'media:read': {
    what: 'Allows reading current media state (track, progress, playing).',
    chain:
      '1) App queries media state.\n2) Read-only session info.\n3) Used for lyrics/visualizers.',
    frontend: 'In-app “now playing” UI.',
    notes: 'Does not change the queue (that is control).',
  },
  'media:audio': {
    what: 'Allows playing package/blob/data audio inside the sandbox.',
    chain:
      '1) App starts audio.\n2) Only allowed sources play.\n3) Not full arbitrary remote control.',
    frontend: 'In-app SFX / audio clips.',
    notes: 'Different from media:control — focuses on sandboxed sources.',
  },
  'component:theme': {
    what: 'Allows registering custom theme styles for the site.',
    chain:
      '1) App registers a theme.\n2) Theme list gains an entry.\n3) Choosing it changes global look.',
    frontend: 'Appearance / theme settings.',
    notes: 'Site-wide visual impact — trust the source.',
  },
  'component:agent': {
    what: 'Allows registering capabilities with the Arael Agent.',
    chain:
      '1) App registers agent tools.\n2) Assistant can invoke them.\n3) Chat may trigger those actions.',
    frontend: 'Arael panel and agent-driven actions.',
    notes: 'Hooks into the global assistant — very high privilege.',
  },
  'shortcut:register': {
    what: 'Allows registering keyboard shortcuts.',
    chain:
      '1) App declares shortcuts.\n2) Keys are handled when focused.\n3) App actions fire.',
    frontend: 'Global/page hotkeys while browsing.',
    notes: 'May conflict with browser or other apps.',
  },
  'event:publish': {
    what: 'Allows publishing events on the system bus.',
    chain:
      '1) App publishes an event.\n2) Subscribers handle it.\n3) Enables cross-module reactions.',
    frontend: 'UI refreshes / notifications driven by events.',
    notes: 'Use agreed event names; avoid noise.',
  },
  'event:subscribe': {
    what: 'Allows subscribing to system events.',
    chain:
      '1) App subscribes.\n2) Publishes deliver callbacks.\n3) App updates its state.',
    frontend: 'Auto-refreshing parts of the app.',
    notes: 'Lower risk than publish.',
  },
  'scheduler:register': {
    what: 'Allows registering scheduled/background tasks.',
    chain:
      '1) App registers jobs.\n2) Host fires them on schedule.\n3) Jobs use other granted APIs.',
    frontend: 'Background sync / reminders.',
    notes: 'Aggressive schedules waste resources; clean up on uninstall.',
  },
  'speech:tts': {
    what: 'Allows text-to-speech.',
    chain:
      '1) App submits text.\n2) Site TTS config synthesizes audio.\n3) Plays or returns audio.',
    frontend: 'In-app “read aloud”.',
    notes: 'Needs TTS configured; may use quota.',
  },
  'speech:asr': {
    what: 'Allows speech-to-text recognition.',
    chain:
      '1) App captures/uploads audio.\n2) ASR service transcribes.\n3) Text returns to the app.',
    frontend: 'In-app voice input.',
    notes: 'Usually needs microphone; privacy-sensitive.',
  },
  'tappList:read': {
    what: 'Allows reading the installed Tapp list and basic metadata.',
    chain:
      '1) App queries list APIs.\n2) Receives install summaries.\n3) Used for display or cross-app flows.',
    frontend: 'In-app “installed apps” views.',
    notes: 'Can reveal which apps you installed.',
  },
  'tappList:manage': {
    what: 'Allows installing, updating, starting/stopping, or uninstalling Tapps.',
    chain:
      '1) App calls manage APIs.\n2) Install state changes.\n3) Lists and run state update.',
    frontend: 'Tapp list and detail run/install controls.',
    notes: 'Among the highest-risk grants.',
  },
  'brew:read': {
    what: 'Allows reading Brew sources and articles.',
    chain:
      '1) App requests Brew data.\n2) Reads site subscriptions.\n3) Renders reading UI.',
    frontend: 'Brew module and in-app readers.',
    notes: 'Read-only; cannot change read state, favorite, or write comments.',
  },
  'brew:readStatus': {
    what: "Allows marking the current user's articles read/unread/all-read.",
    chain:
      "1) App updates item read state.\n2) Changes only the current user's own state.\n3) Filters/badges may change.",
    frontend: 'Brew read state.',
    notes: 'Only affects your own reading state, not shared content.',
  },
  'brew:favorite': {
    what: 'Allows starring/unstarring articles for the current user.',
    chain:
      "1) App updates item star state.\n2) Changes only the current user's own favorites.\n3) Starred filter updates.",
    frontend: 'Brew favorites/stars.',
    notes: 'Only affects your own favorites, not shared content.',
  },
  'brew:commentWrite': {
    what: 'Allows creating, editing, and deleting Brew comments and replies.',
    chain:
      '1) App posts comments.\n2) Comments attach to articles.\n3) Others may see them per policy.',
    frontend: 'Brew article comments.',
    notes: 'Public expression — follow content norms; delegated by admins only by default.',
  },
  'brew:manage': {
    what: 'Allows managing Brew sources and advanced settings.',
    chain:
      '1) App calls source-management APIs.\n2) Subscription set changes.\n3) Fetch/list update.',
    frontend: 'Brew source manager, OPML, discover.',
    notes: 'Can add/remove feeds site-wide for Brew.',
  },
  'federation:read': {
    what: 'Allows reading federation rooms, channels, and members.',
    chain:
      '1) App queries federation APIs.\n2) Returns data allowed by trust policy.\n3) Renders federation state.',
    frontend: 'Federation pages / in-app views.',
    notes: 'Visibility still limited by trust rules.',
  },
  'federation:write': {
    what: 'Allows creating or updating federation resources.',
    chain:
      '1) App writes federation APIs.\n2) Local (and possibly remote) resources change.\n3) Members see structure updates.',
    frontend: 'Federation create/manage flows.',
    notes: 'May sync to remote instances.',
  },
  'federation:message': {
    what: 'Allows sending and receiving federation messages.',
    chain:
      '1) App sends/receives messages.\n2) Delivery queue handles transport.\n3) Peers may receive content.',
    frontend: 'Federation messaging UI.',
    notes: 'Content can leave the site — privacy/abuse risk.',
  },
  'federation:trust': {
    what: 'Allows managing federation trust relationships.',
    chain:
      '1) App adjusts trust policy.\n2) Affects accept/reject of peers.\n3) Filtering and delivery change.',
    frontend: 'Config → Federation trust/policy.',
    notes: 'Misconfig can break federation or trust bad peers.',
  },
  'federation:files': {
    what: 'Allows federation file transfer capabilities.',
    chain:
      '1) App uses file APIs.\n2) Files go through federation file channel.\n3) Arrive remotely or download locally.',
    frontend: 'Federation attachments/files.',
    notes: 'Size/type limits may apply; malware risk.',
  },
}

const ja: TappPermissionGuides = {
  'widget:register': {
    what: 'ホームのグリッドにカスタムウィジェットを登録できます。',
    chain:
      '① 権限が付与される。\n② 登録 API でホームにカードが出る。\n③ 管理者級で、訪問者全員に見える。',
    frontend: 'ホームの Widget グリッド、アプリ内の登録/解除。',
    notes: '信頼できるアプリだけ。悪質なカードが全サイトに出ます。',
  },
  'platform:read': {
    what: '接続済みプラットフォームの既存データを読めます。',
    chain:
      '① 読み取り API。\n② 同期済みデータから取得。\n③ 設定自体は変えない。',
    frontend: 'アプリ内の統計・一覧・進捗。',
    notes: '未設定のプラットフォームは空のことが多いです。',
  },
  'platform:write': {
    what: 'プラットフォームデータへの書き込み・更新ができます。',
    chain:
      '① 書き込み API。\n② サイトの保存領域へ。\n③ レポート等の表示も変わり得る。',
    frontend: 'アプリの取り込み/同期、その後の本サイト表示。',
    notes: '統計を汚す可能性がある高リスク権限です。',
  },
  'platform:register': {
    what: 'カスタムデータプラットフォーム種別を登録できます。',
    chain:
      '① 定義を登録。\n② 接続可能一覧に出る。\n③ その後設定・同期できる。',
    frontend: '設定の「プラットフォーム」。',
    notes: 'サイト級の拡張。出所を確認してください。',
  },
  'analytics:read': {
    what: 'サイト訪問統計の集計（本日/期間の訪問、人気ページ、流入元など）を読めます。',
    chain:
      '① 本権限が付与される。\n② Tapp.analytics で集計サマリーを取得。\n③ 集計のみで、訪問者指紋や生ログは含まない。',
    frontend: 'アプリ内の統計カード等。データ源はサイト分析。',
    notes: '読み取り専用の集計。インポート/エクスポートや AI 利用明細は対象外。',
  },
  'ai:generate': {
    what: 'AI による文章などの生成を呼べます。',
    chain:
      '① 生成リクエスト。\n② ゲートウェイとクォータ。\n③ 結果がアプリへ。',
    frontend: '生成ボタン、AI 設定とクォータ。',
    notes: 'クォータを消費します。',
  },
  'ai:analyze': {
    what: 'AI による分析・解釈を呼べます。',
    chain:
      '① 分析対象を送る。\n② 分析呼び出しとクォータ。\n③ 結果を表示。',
    frontend: 'アプリ内の分析機能。',
    notes: '大きな入力は遅く失敗することがあります。',
  },
  'ai:chat': {
    what: 'AI との対話（チャット）を呼べます。',
    chain:
      '① セッション開始。\n② チャット経路。\n③ 履歴はアプリ側のことが多い。',
    frontend: 'アプリ内チャット（全体 Arael とは別）。',
    notes: '往復ごとにクォータを使い得ます。',
  },
  'ai:image': {
    what: 'AI 画像生成を呼べます。',
    chain:
      '① 画像リクエスト。\n② 画像モデル設定。\n③ URL/データを返す。',
    frontend: 'アプリ内の画像生成。',
    notes: '遅延・コスト大。画像能力の設定が必要。',
  },
  'report:read': {
    what: '生成済みレポートを読めます。',
    chain:
      '① レポート取得。\n② 保存領域から読む。\n③ アプリで表示。',
    frontend: 'レポートページ、アプリ内ビュー。',
    notes: '新規作成はこの権限だけではしません。',
  },
  'report:write': {
    what: 'レポートの作成・更新・削除ができます。',
    chain:
      '① 書き込み API。\n② 一覧/内容が変わる。\n③ レポートページに反映。',
    frontend: 'レポート一覧と詳細。',
    notes: 'ユーザー向けレポートを消せる高リスクです。',
  },
  'storage:read': {
    what: 'このアプリ専用のキー値ストレージを使えます。',
    chain:
      '① storage API。\n② アプリ単位で隔離。\n③ 削除時に消すか選べる。',
    frontend: '設定・下書き・状態の保存。',
    notes: '信頼できないアプリに秘密を置かないでください。',
  },
  'storage:write': {
    what: 'このアプリ専用のキー値ストレージへ書き込めます。',
    chain: '① 書き込み API。\n② アプリ単位で隔離。\n③ 次回のアプリ状態に反映。',
    frontend: '設定・下書き・永続状態の保存。',
    notes: '既定では無効。信頼できるアプリだけに下放してください。',
  },
  'ui:notification': {
    what: 'ユーザーへシステム通知を送れます。',
    chain:
      '① 通知 API。\n② センター/島/トースト等。\n③ ユーザー設定が優先。',
    frontend: '通知センター、Toast、許可時のブラウザ通知。',
    notes: '連投は避けて。ユーザーは遮断できます。',
  },
  'ui:fullscreen': {
    what: 'ブラウザの全画面表示を要求できます。',
    chain:
      '① 全画面 API。\n② ブラウザが確認することがある。\n③ いつでも解除可能。',
    frontend: '実行ページの全画面。',
    notes: '多くの場合ユーザー操作が必要です。',
  },
  'ui:theme': {
    what: '現在のサイトテーマ（明暗・主色など）を読めます。',
    chain:
      '① テーマ API。\n② トークン取得。\n③ 見た目合わせのみ（変更はしない）。',
    frontend: 'アプリ UI の配色合わせ。',
    notes: '読み取り専用。変更は設定の外観から。',
  },
  'ui:confirm': {
    what: '確認ダイアログを出せます。',
    chain:
      '① 確認要求。\n② ユーザーが OK/キャンセル。\n③ 結果がアプリへ。',
    frontend: '実行中のモーダル確認。',
    notes: '多用すると迷惑になります。',
  },
  'ui:openUrl': {
    what: 'Manifest openUrls で宣言したリンクのみ、ホスト経由で新しいタブを開けます（未宣言 URL は拒否）。',
    chain:
      '① openUrls と本権限を宣言しインストール承認。\n② Tapp.ui.openUrl({ id }) を呼ぶ（match 内の path/query 可）。\n③ ホストが allowlist を検証して開く。追加の確認ダイアログはなし。',
    frontend: 'アプリ内の外部リンクボタン/カード。詳細ページの権限説明。',
    notes:
      '宣言済みリンクのみ。サンドボックスの window.open は無効のまま。インストール時にドメインを確認。',
  },
  'network:fetch': {
    what: '宣言的 HTTP と、遠隔の画像・音声などの読み込みができます。',
    chain:
      '① エンドポイント/資源指定。\n② ホストが検証・中継。\n③ 応答がサンドボックスへ。',
    frontend: '外部データ、カバー、メディア。',
    notes: 'プロキシ/CORS 制限あり。外部接続リスク。',
  },
  'media:control': {
    what: '再生の操作（再生・一時停止・送りなど）ができます。',
    chain:
      '① 制御 API。\n② サイトの再生状態に作用。\n③ コントロールパネルの音楽も変わり得る。',
    frontend: '全体の音楽プレイヤー、アプリ内操作。',
    notes: 'ユーザーが聴いている内容を中断し得ます。',
  },
  'media:read': {
    what: '現在の再生状態（曲・進捗・再生中か）を読めます。',
    chain:
      '① 状態照会。\n② 読み取りのみ。\n③ 歌詞や可視化に利用。',
    frontend: '「再生中」表示。',
    notes: 'キュー変更は control 側です。',
  },
  'media:audio': {
    what: 'サンドボックス内でパッケージ/blob/data 音声を再生できます。',
    chain:
      '① 再生開始。\n② 許可された音源のみ。\n③ 任意外部の完全制御ではない。',
    frontend: '効果音・短い音声。',
    notes: 'media:control とは役割が違います。',
  },
  'component:theme': {
    what: 'カスタムテーマを登録し、見た目の選択肢を増やせます。',
    chain:
      '① テーマ登録。\n② 一覧に追加。\n③ 選ぶと全体の見た目が変わる。',
    frontend: '外観/テーマ設定。',
    notes: '全サイトに影響。出所を確認。',
  },
  'component:agent': {
    what: 'Arael Agent に能力（ツール）を登録できます。',
    chain:
      '① 能力登録。\n② アシスタントが呼べる。\n③ 会話から動作が起き得る。',
    frontend: 'Arael パネルと関連操作。',
    notes: '全体アシスタントへの接続。非常に強い権限。',
  },
  'shortcut:register': {
    what: 'キーボードショートカットを登録できます。',
    chain:
      '① ショートカット宣言。\n② フォーカス時にキー処理。\n③ アプリの動作が走る。',
    frontend: '閲覧中のホットキー。',
    notes: 'ブラウザや他アプリと衝突することがあります。',
  },
  'event:publish': {
    what: 'システムイベントバスへイベントを発行できます。',
    chain:
      '① 発行。\n② 購読側が処理。\n③ モジュール横断の連携。',
    frontend: 'イベント駆動の更新・通知。',
    notes: '約束したイベント名を使い、乱発しないで。',
  },
  'event:subscribe': {
    what: 'システムイベントを購読できます。',
    chain:
      '① 購読。\n② 発行時にコールバック。\n③ アプリが状態更新。',
    frontend: '自動更新される部分。',
    notes: 'publish よりリスクは低いです。',
  },
  'scheduler:register': {
    what: '定時・バックグラウンドタスクを登録できます。',
    chain:
      '① ジョブ登録。\n② ホストがスケジュール実行。\n③ 許可された他 API を使える。',
    frontend: '定時同期・リマインダ。',
    notes: '高頻度は負荷。削除時は掃除を。',
  },
  'speech:tts': {
    what: 'テキスト読み上げ（TTS）を使えます。',
    chain:
      '① テキスト送信。\n② サイト TTS 設定で合成。\n③ 再生または音声返却。',
    frontend: '読み上げ機能。',
    notes: 'TTS 設定とクォータが必要になることがあります。',
  },
  'speech:asr': {
    what: '音声認識（ASR）を使えます。',
    chain:
      '① 音声の取得/送信。\n② 認識サービス。\n③ テキストが返る。',
    frontend: '音声入力。',
    notes: 'マイク権限とプライバシーに注意。',
  },
  'tappList:read': {
    what: 'インストール済み Tapp 一覧と基本情報を読めます。',
    chain:
      '① 一覧 API。\n② 要約を取得。\n③ 表示や連携に利用。',
    frontend: '「インストール済み」系 UI。',
    notes: 'どのアプリを入れたかが分かることがあります。',
  },
  'tappList:manage': {
    what: 'Tapp のインストール・更新・起動停止・削除ができます。',
    chain:
      '① 管理 API。\n② インストール状態が変わる。\n③ 一覧と実行状態が更新。',
    frontend: 'Tapp 一覧・詳細の操作。',
    notes: '最も危険な権限の一つです。',
  },
  'brew:read': {
    what: 'Brew の購読・記事・コメントを読めます。',
    chain:
      '① Brew データ取得。\n② サイトの購読から読む。\n③ アプリで表示。',
    frontend: 'Brew とアプリ内リーダー。',
    notes: '読み取り専用。既読変更・お気に入り・コメント書き込みは不可。',
  },
  'brew:readStatus': {
    what: '現在のユーザーの既読/未読/すべて既読を変更できます。',
    chain:
      '① 状態更新。\n② 自分の状態だけ変わる。\n③ フィルタやバッジが変わり得る。',
    frontend: 'Brew 既読状態。',
    notes: '自分だけの読書状態に影響し、共有内容には影響しません。',
  },
  'brew:favorite': {
    what: '現在のユーザーの記事をスター/スター解除できます。',
    chain:
      '① スター更新。\n② 自分のお気に入りだけ変わる。\n③ スター絞り込みが更新。',
    frontend: 'Brew お気に入り/スター。',
    notes: '自分だけのお気に入りに影響し、共有内容には影響しません。',
  },
  'brew:commentWrite': {
    what: 'Brew コメントと返信の作成・編集・削除ができます。',
    chain:
      '① コメント投稿。\n② 記事に紐づく。\n③ 方針により他者も見る。',
    frontend: 'Brew のコメント欄。',
    notes: '公開表現のマナーに注意。デフォルトでは管理者のみ下放可能。',
  },
  'brew:manage': {
    what: 'Brew の購読源と高度な設定を管理できます。',
    chain:
      '① 源管理 API。\n② 購読集合が変わる。\n③ 取得と一覧が更新。',
    frontend: '源管理、OPML、発見。',
    notes: 'サイトの Brew 内容プールに影響。',
  },
  'federation:read': {
    what: 'フェデレーションの部屋・チャンネル・メンバーを読めます。',
    chain:
      '① 照会 API。\n② 信頼ポリシー内のデータを返す。\n③ 状態表示。',
    frontend: 'フェデレーション画面。',
    notes: '見える範囲は信頼設定に依存。',
  },
  'federation:write': {
    what: 'フェデレーション資源の作成・更新ができます。',
    chain:
      '① 書き込み API。\n② 自サイト（と相手）が変わり得る。\n③ 構成が更新される。',
    frontend: '作成・管理フロー。',
    notes: '外部インスタンスへ同期することがあります。',
  },
  'federation:message': {
    what: 'フェデレーションメッセージの送受信ができます。',
    chain:
      '① 送受信。\n② 配送キュー。\n③ 相手が内容を受け取る可能性。',
    frontend: 'メッセージ UI。',
    notes: 'サイト外へ出る可能性。プライバシーに注意。',
  },
  'federation:trust': {
    what: 'フェデレーションの信頼関係を管理できます。',
    chain:
      '① 信頼ポリシー変更。\n② 受け入れ/拒否に影響。\n③ フィルタと配送が変わる。',
    frontend: '設定のフェデレーション信頼。',
    notes: '誤設定で接続不能や誤信が起き得ます。',
  },
  'federation:files': {
    what: 'フェデレーションのファイル転送を使えます。',
    chain:
      '① ファイル API。\n② ファイル経路。\n③ 相手へ到達またはローカル保存。',
    frontend: '添付/ファイル機能。',
    notes: 'サイズ制限あり。悪意あるファイルに注意。',
  },
}

export function getTappPermissionGuides(locale: Locale): TappPermissionGuides {
  if (locale === 'zh-CN') return zh
  if (locale === 'ja-JP') return ja
  return en
}

export function getTappPermissionGuide(
  locale: Locale,
  permission: TappPermission,
): SettingGuideEntry {
  return getTappPermissionGuides(locale)[permission]
}
