/**
 * Tapp Permission Controller - 权限控制器
 * 管理 Tapp 的权限请求和验证
 */

import type {
  AIQuotaStatus,
  PermissionLevel,
  TappInstance,
  TappManifest,
  TappPermission,
  UserRole,
} from '../types'

/** 权限描述信息 */
const PERMISSION_INFO: Record<TappPermission, {
  title: string
  description: string
  level: PermissionLevel
  dangerous?: boolean
}> = {
  'widget:register': {
    title: '注册小组件',
    description: '允许此应用向系统注册自定义小组件',
    level: 'basic',
  },
  'platform:read': {
    title: '读取平台数据',
    description: '允许此应用读取你的平台数据（游戏、视频、音乐等）',
    level: 'basic',
  },
  'platform:write': {
    title: '写入平台数据',
    description: '允许此应用向资料库添加新数据',
    level: 'elevated',
  },
  'platform:register': {
    title: '注册自定义平台',
    description: '允许此应用注册新的数据源平台',
    level: 'elevated',
  },
  'ai:generate': {
    title: 'AI 文本生成',
    description: '允许此应用使用 AI 生成文本内容',
    level: 'elevated',
  },
  'ai:analyze': {
    title: 'AI 数据分析',
    description: '允许此应用使用 AI 分析数据',
    level: 'elevated',
  },
  'ai:chat': {
    title: 'AI 对话',
    description: '允许此应用与 AI 进行多轮对话',
    level: 'elevated',
  },
  'ai:image': {
    title: 'AI 图片生成',
    description: '允许此应用使用 AI 生成图片内容',
    level: 'elevated',
  },
  'report:read': {
    title: '读取报告',
    description: '允许此应用读取你的分析报告',
    level: 'basic',
  },
  'report:write': {
    title: '写入报告',
    description: '允许此应用创建、更新和删除报告',
    level: 'elevated',
  },
  'storage': {
    title: '本地存储',
    description: '允许此应用在本地存储数据',
    level: 'basic',
  },
  'ui:notification': {
    title: '显示通知',
    description: '允许此应用显示通知消息',
    level: 'basic',
  },
  'ui:fullscreen': {
    title: '全屏模式',
    description: '允许此应用请求全屏显示',
    level: 'basic',
  },
  'ui:theme': {
    title: '读取主题',
    description: '允许此应用读取当前主题设置',
    level: 'basic',
  },
  'ui:confirm': {
    title: '确认对话框',
    description: '允许此应用显示确认对话框',
    level: 'basic',
  },
  'network:fetch': {
    title: '网络请求',
    description: '允许此应用通过代理发送 HTTP 请求',
    level: 'elevated',
  },
  'media:control': {
    title: '媒体控制',
    description: '允许此应用控制媒体播放器（播放、暂停、切换曲目等）',
    level: 'elevated',
  },
  'media:read': {
    title: '读取媒体状态',
    description: '允许此应用读取当前媒体播放状态',
    level: 'basic',
  },
  'component:theme': {
    title: '注册主题',
    description: '允许此应用注册自定义主题样式',
    level: 'elevated',
  },
  'component:agent': {
    title: '注册 Agent',
    description: '允许此应用注册 AI Agent 能力',
    level: 'privileged',
  },
  'shortcut:register': {
    title: '注册快捷键',
    description: '允许此应用注册键盘快捷键',
    level: 'elevated',
  },
  'event:publish': {
    title: '发布事件',
    description: '允许此应用发布系统事件',
    level: 'elevated',
  },
  'event:subscribe': {
    title: '订阅事件',
    description: '允许此应用订阅系统事件',
    level: 'basic',
  },
  'scheduler:register': {
    title: '注册定时任务',
    description: '允许此应用注册后台定时任务',
    level: 'elevated',
  },
}

/**
 * 基于用户角色的 AI 配额配置
 * 管理员无限制，用户和游客使用后端配置的限额
 *
 * 注意：这些只是前端的默认值，实际限额由后端 API 返回
 */
const DEFAULT_AI_QUOTA_BY_ROLE = {
  admin: {
    dailyCalls: Infinity,
    dailyTokens: Infinity,
    cooldownSeconds: 0,
    maxPromptLength: 10000,
    maxCompletionTokens: 8000,
    unlimited: true,
  },
  user: {
    dailyCalls: 50,
    dailyTokens: 20000,
    cooldownSeconds: 5,
    maxPromptLength: 2000,
    maxCompletionTokens: 2000,
    unlimited: false,
  },
  guest: {
    dailyCalls: 10,
    dailyTokens: 5000,
    cooldownSeconds: 10,
    maxPromptLength: 1000,
    maxCompletionTokens: 1000,
    unlimited: false,
  },
}

/**
 * 权限控制器类
 */
export class TappPermissionController {
  private tappInstance: TappInstance
  private aiQuotaUsage: {
    calls: number
    tokens: number
    lastReset: Date
    lastCall: Date | null
  }

  constructor(tappInstance: TappInstance) {
    this.tappInstance = tappInstance
    this.aiQuotaUsage = {
      calls: tappInstance.quotaUsage?.ai.dailyCalls || 0,
      tokens: tappInstance.quotaUsage?.ai.dailyTokens || 0,
      lastReset: new Date(tappInstance.quotaUsage?.ai.lastReset || Date.now()),
      lastCall: null,
    }
  }

  /**
   * 获取当前用户角色
   */
  getUserRole(): UserRole {
    return this.tappInstance.userRole || 'guest'
  }

  /**
   * 根据用户角色获取允许的权限级别
   * - guest: 无权限（只能查看）
   * - user: 只能使用 basic 权限
   * - admin: 可使用所有权限
   */
  getAllowedPermissionLevels(): PermissionLevel[] {
    const role = this.getUserRole()
    switch (role) {
      case 'admin':
        return ['public', 'basic', 'elevated', 'privileged']
      case 'user':
        return ['public', 'basic']
      case 'guest':
      default:
        return ['public']
    }
  }

  /**
   * 检查权限是否在用户角色允许范围内
   */
  isPermissionAllowedForRole(permission: TappPermission): boolean {
    const info = PERMISSION_INFO[permission]
    if (!info)
      return false

    const allowedLevels = this.getAllowedPermissionLevels()
    return allowedLevels.includes(info.level)
  }

  /**
   * 检查是否拥有权限
   *
   * 后端已经根据权限下放配置过滤了 grantedPermissions，
   * 所以前端只需要检查权限是否在列表中即可，无需再次验证角色权限级别。
   */
  hasPermission(permission: TappPermission): boolean {
    // 只检查是否在授权列表中
    // 后端已根据用户角色和权限下放配置过滤，这里不再做二次检查
    return this.tappInstance.grantedPermissions.includes(permission)
  }

  /**
   * 检查是否拥有所有权限
   */
  hasAllPermissions(permissions: TappPermission[]): boolean {
    return permissions.every(p => this.hasPermission(p))
  }

  /**
   * 检查是否拥有任一权限
   */
  hasAnyPermission(permissions: TappPermission[]): boolean {
    return permissions.some(p => this.hasPermission(p))
  }

  /**
   * 获取用户角色实际可用的权限列表
   *
   * 由于后端已根据权限下放配置过滤了 grantedPermissions，
   * 这里直接返回所有已授权权限。
   */
  getEffectivePermissions(): TappPermission[] {
    return [...this.tappInstance.grantedPermissions]
  }

  /**
   * 获取因角色限制而不可用的权限列表
   *
   * 由于后端已经做了权限过滤，这里返回空数组。
   * 保留此方法是为了向后兼容。
   */
  getRestrictedPermissions(): TappPermission[] {
    return []
  }

  /**
   * 获取权限信息
   */
  getPermissionInfo(permission: TappPermission) {
    return PERMISSION_INFO[permission]
  }

  /**
   * 获取所有已授权权限的详细信息
   */
  getGrantedPermissionsInfo() {
    return this.tappInstance.grantedPermissions.map(p => ({
      permission: p,
      ...PERMISSION_INFO[p],
    }))
  }

  /**
   * 验证 Manifest 中的权限是否合法
   */
  static validateManifestPermissions(manifest: TappManifest): {
    valid: boolean
    errors: string[]
    warnings: string[]
  } {
    const errors: string[] = []
    const warnings: string[] = []

    // 检查必需权限
    for (const permission of manifest.permissions) {
      if (!PERMISSION_INFO[permission]) {
        errors.push(`未知权限: ${permission}`)
      }
    }

    // 检查可选权限
    if (manifest.optionalPermissions) {
      for (const permission of manifest.optionalPermissions) {
        if (!PERMISSION_INFO[permission]) {
          errors.push(`未知可选权限: ${permission}`)
        }
        // 可选权限不应该在必需权限中
        if (manifest.permissions.includes(permission)) {
          warnings.push(`权限 ${permission} 同时出现在必需和可选列表中`)
        }
      }
    }

    // 检查敏感权限组合
    if (
      manifest.permissions.includes('platform:write')
      && manifest.permissions.includes('ai:generate')
    ) {
      warnings.push('同时请求写入数据和 AI 生成权限，请确保应用来源可信')
    }

    return {
      valid: errors.length === 0,
      errors,
      warnings,
    }
  }

  /**
   * 获取 AI 配额限制（基于用户角色）
   *
   * 管理员无限制，普通用户和游客使用系统配置的限额
   * 注意：实际限额应该从后端 API 获取，这里只是前端的默认值
   *
   * @deprecated 应用不再声明 aiQuota，改用基于用户角色的统一限额
   */
  getAIQuotaLimits() {
    const role = this.getUserRole()
    return DEFAULT_AI_QUOTA_BY_ROLE[role] || DEFAULT_AI_QUOTA_BY_ROLE.guest
  }

  /**
   * 获取 AI 配额状态（基于用户角色）
   */
  getAIQuotaStatus(): AIQuotaStatus {
    const limits = this.getAIQuotaLimits()
    const role = this.getUserRole()
    const now = new Date()

    // 管理员无限制
    if (limits.unlimited) {
      const nextReset = new Date()
      nextReset.setDate(nextReset.getDate() + 1)
      nextReset.setHours(0, 0, 0, 0)

      return {
        daily: {
          limit: Infinity,
          used: this.aiQuotaUsage.calls,
          resetsAt: nextReset.toISOString(),
        },
        tokens: {
          limit: Infinity,
          used: this.aiQuotaUsage.tokens,
          resetsAt: nextReset.toISOString(),
        },
        cooldown: {
          required: 0,
          remaining: 0,
        },
        restricted: false,
        unlimited: true,
        userRole: role,
      }
    }

    // 检查是否需要重置（每日重置）
    const resetTime = new Date(this.aiQuotaUsage.lastReset)
    resetTime.setHours(0, 0, 0, 0)
    const today = new Date()
    today.setHours(0, 0, 0, 0)

    if (today > resetTime) {
      this.aiQuotaUsage.calls = 0
      this.aiQuotaUsage.tokens = 0
      this.aiQuotaUsage.lastReset = today
    }

    // 计算冷却时间
    let cooldownRemaining = 0
    if (this.aiQuotaUsage.lastCall) {
      const elapsed = (now.getTime() - this.aiQuotaUsage.lastCall.getTime()) / 1000
      cooldownRemaining = Math.max(0, limits.cooldownSeconds - elapsed)
    }

    // 计算下次重置时间
    const nextReset = new Date(today)
    nextReset.setDate(nextReset.getDate() + 1)

    const isRestricted = this.aiQuotaUsage.calls >= limits.dailyCalls
      || this.aiQuotaUsage.tokens >= limits.dailyTokens

    return {
      daily: {
        limit: limits.dailyCalls,
        used: this.aiQuotaUsage.calls,
        resetsAt: nextReset.toISOString(),
      },
      tokens: {
        limit: limits.dailyTokens,
        used: this.aiQuotaUsage.tokens,
        resetsAt: nextReset.toISOString(),
      },
      cooldown: {
        required: limits.cooldownSeconds,
        remaining: Math.ceil(cooldownRemaining),
      },
      restricted: isRestricted,
      restrictionReason: this.aiQuotaUsage.calls >= limits.dailyCalls
        ? '已达到每日调用次数上限'
        : this.aiQuotaUsage.tokens >= limits.dailyTokens
          ? '已达到每日 Token 上限'
          : undefined,
      unlimited: false,
      userRole: role,
    }
  }

  /**
   * 检查是否可以进行 AI 调用
   *
   * 管理员始终可以调用（无限制）
   * 普通用户和游客受配额限制
   */
  canMakeAICall(): { allowed: boolean, reason?: string } {
    // 检查是否有 AI 权限
    if (!this.hasAnyPermission(['ai:generate', 'ai:analyze', 'ai:chat', 'ai:image'])) {
      return { allowed: false, reason: '没有 AI 权限' }
    }

    // 管理员无限制
    if (this.getUserRole() === 'admin') {
      return { allowed: true }
    }

    const status = this.getAIQuotaStatus()

    if (status.restricted) {
      return { allowed: false, reason: status.restrictionReason }
    }

    if (status.cooldown.remaining > 0) {
      return {
        allowed: false,
        reason: `请等待 ${status.cooldown.remaining} 秒后再试`,
      }
    }

    return { allowed: true }
  }

  /**
   * 记录 AI 调用
   */
  recordAICall(tokensUsed: number): void {
    this.aiQuotaUsage.calls += 1
    this.aiQuotaUsage.tokens += tokensUsed
    this.aiQuotaUsage.lastCall = new Date()
  }

  /**
   * 验证 prompt 内容（内容审核 + Prompt 注入防护）
   *
   * 检测以下类型的攻击：
   * 1. 角色覆盖攻击（尝试覆盖系统 prompt）
   * 2. 指令注入（尝试绕过 AI 限制）
   * 3. 敏感信息探测（API 密钥、密码等）
   * 4. 外部资源加载（URL 注入）
   * 5. Unicode/编码绕过攻击
   * 6. 多语言混淆攻击
   */
  validatePrompt(prompt: string): { valid: boolean, reason?: string, severity?: 'low' | 'medium' | 'high' } {
    const limits = this.getAIQuotaLimits()

    // 长度检查
    if (prompt.length > limits.maxPromptLength) {
      return {
        valid: false,
        reason: `提示词过长，最大 ${limits.maxPromptLength} 字符`,
        severity: 'low',
      }
    }

    // 预处理：规范化 Unicode 字符（防止同形字符绕过）
    const normalizedPrompt = prompt.normalize('NFKC')

    // 检测隐藏字符和零宽字符（可能用于绕过检测）
    const hiddenCharPattern = /[\u200B-\u200D\u2060\uFEFF\u00AD]/g
    if (hiddenCharPattern.test(prompt)) {
      console.warn('[Tapp Security] Hidden characters detected in prompt')
      return {
        valid: false,
        reason: '提示词包含隐藏字符',
        severity: 'medium',
      }
    }

    // P0: Prompt 注入攻击检测模式（增强版）
    const injectionPatterns: Array<{ pattern: RegExp, reason: string, severity: 'low' | 'medium' | 'high' }> = [
      // 角色覆盖攻击
      { pattern: /ignore\s+(all\s+)?(previous|above|prior)\s+(instructions?|prompts?|rules?)/gi, reason: '检测到角色覆盖攻击尝试', severity: 'high' },
      { pattern: /forget\s+(everything|all|your)\s+(you\s+)?know/gi, reason: '检测到角色覆盖攻击尝试', severity: 'high' },
      { pattern: /you\s+are\s+now\s+(a|an|the)\s+/gi, reason: '检测到角色覆盖攻击尝试', severity: 'high' },
      { pattern: /disregard\s+(all|any|the)\s+(previous|prior|above)/gi, reason: '检测到角色覆盖攻击尝试', severity: 'high' },
      { pattern: /new\s+instructions?:?\s*$/gim, reason: '检测到指令注入尝试', severity: 'high' },
      { pattern: /system\s*prompt:?/gi, reason: '检测到系统提示词覆盖尝试', severity: 'high' },
      { pattern: /\[system\]/gi, reason: '检测到系统标记注入', severity: 'high' },
      { pattern: /\[\[.*\]\]/g, reason: '检测到特殊标记注入', severity: 'medium' },
      { pattern: /\{\{.*\}\}/g, reason: '检测到模板注入尝试', severity: 'medium' },
      { pattern: /<\|.*\|>/g, reason: '检测到特殊分隔符注入', severity: 'high' },

      // 越狱尝试（扩展检测）
      { pattern: /jailbreak/gi, reason: '检测到越狱关键词', severity: 'high' },
      { pattern: /dan\s*mode/gi, reason: '检测到越狱关键词', severity: 'high' },
      { pattern: /developer\s*mode/gi, reason: '检测到越狱关键词', severity: 'medium' },
      { pattern: /bypass\s+(safety|content|filter|restriction)/gi, reason: '检测到绕过安全检测尝试', severity: 'high' },
      { pattern: /pretend\s+(you\s+)?(are|to\s+be)/gi, reason: '检测到角色扮演绕过尝试', severity: 'medium' },
      { pattern: /act\s+as\s+if\s+you\s+(have\s+)?no\s+(restrictions?|limitations?|rules?)/gi, reason: '检测到限制绕过尝试', severity: 'high' },
      { pattern: /\buncensored\b/gi, reason: '检测到限制绕过关键词', severity: 'medium' },
      { pattern: /\bunfiltered\b/gi, reason: '检测到限制绕过关键词', severity: 'medium' },

      // 敏感信息探测
      { pattern: /https?:\/\/\S+/gi, reason: '提示词中不允许包含 URL', severity: 'medium' },
      { pattern: /api[_-]?key/gi, reason: '检测到 API 密钥探测', severity: 'high' },
      { pattern: /\bpassword\b/gi, reason: '检测到密码相关内容', severity: 'medium' },
      { pattern: /\btoken\b/gi, reason: '检测到 Token 相关内容', severity: 'low' },
      { pattern: /\bsecret\b/gi, reason: '检测到密钥相关内容', severity: 'medium' },
      { pattern: /private[_-]?key/gi, reason: '检测到私钥探测', severity: 'high' },
      { pattern: /credentials?/gi, reason: '检测到凭证探测', severity: 'medium' },
      { pattern: /ssh[_-]?key/gi, reason: '检测到 SSH 密钥探测', severity: 'high' },
      { pattern: /bearer\s+[\w-]+/gi, reason: '检测到 Bearer Token 探测', severity: 'high' },

      // 代码执行尝试（扩展检测）
      { pattern: /eval\s*\(/gi, reason: '检测到代码执行尝试', severity: 'high' },
      { pattern: /exec\s*\(/gi, reason: '检测到代码执行尝试', severity: 'high' },
      { pattern: /__import__/gi, reason: '检测到代码执行尝试', severity: 'high' },
      { pattern: /subprocess/gi, reason: '检测到系统命令执行尝试', severity: 'high' },
      { pattern: /os\.system/gi, reason: '检测到系统命令执行尝试', severity: 'high' },
      { pattern: /child_process/gi, reason: '检测到进程创建尝试', severity: 'high' },
      { pattern: /spawn\s*\(/gi, reason: '检测到进程创建尝试', severity: 'high' },
      { pattern: /\$\(.*\)/g, reason: '检测到命令替换尝试', severity: 'high' },

      // SQL 注入尝试
      { pattern: /union\s+select/gi, reason: '检测到 SQL 注入尝试', severity: 'high' },
      { pattern: /;\s*drop\s+/gi, reason: '检测到 SQL 注入尝试', severity: 'high' },
      { pattern: /--\s*$/gm, reason: '检测到 SQL 注释注入', severity: 'medium' },
    ]

    for (const { pattern, reason, severity } of injectionPatterns) {
      // 重置正则表达式的 lastIndex
      pattern.lastIndex = 0
      if (pattern.test(normalizedPrompt)) {
        console.warn(`[Tapp Security] Prompt injection detected: ${reason} (severity: ${severity})`)
        return {
          valid: false,
          reason: '提示词包含不允许的内容',
          severity,
        }
      }
    }

    // 检测重复字符（可能的 token 溢出攻击）
    const repeatedCharPattern = /(.)\1{50,}/g
    if (repeatedCharPattern.test(prompt)) {
      return {
        valid: false,
        reason: '提示词格式异常',
        severity: 'medium',
      }
    }

    // 检测过多的特殊字符（可能的格式注入）
    const specialCharCount = (prompt.match(/[<>{}[\]\\|`~^]/g) || []).length
    if (specialCharCount > prompt.length * 0.3) {
      return {
        valid: false,
        reason: '提示词包含过多特殊字符',
        severity: 'medium',
      }
    }

    // 检测 Base64 编码的内容（可能用于绕过检测）
    const base64Pattern = /^[A-Z0-9+/]{50,}={0,2}$/i
    const words = prompt.split(/\s+/)
    for (const word of words) {
      if (base64Pattern.test(word)) {
        try {
          const decoded = atob(word)
          // 递归检查解码后的内容
          const decodedCheck = this.validatePrompt(decoded)
          if (!decodedCheck.valid) {
            console.warn('[Tapp Security] Encoded malicious content detected')
            return {
              valid: false,
              reason: '检测到编码绕过尝试',
              severity: 'high',
            }
          }
        }
        catch {
          // 不是有效的 Base64，忽略
        }
      }
    }

    // 检测异常的非 ASCII 字符比例（可能是混淆攻击）
    const nonAsciiCount = (prompt.match(/[^\x00-\x7F]/g) || []).length
    const asciiCount = prompt.length - nonAsciiCount
    // 如果非 ASCII 字符过少但存在，可能是同形字符攻击
    if (nonAsciiCount > 0 && nonAsciiCount < 5 && asciiCount > 50) {
      // 检查是否是常见的同形字符（西里尔字母等）
      const homoglyphs = /[\u0430\u0435\u043E\u0440\u0441\u0443\u0445\u0410\u0412\u0415\u041A\u041C\u041D\u041E\u0420\u0421\u0422\u0425]/g
      if (homoglyphs.test(prompt)) {
        console.warn('[Tapp Security] Homoglyph attack detected')
        return {
          valid: false,
          reason: '检测到同形字符混淆攻击',
          severity: 'medium',
        }
      }
    }

    return { valid: true }
  }

  /**
   * 清理和规范化用户输入
   * 用于在验证前预处理输入
   */
  sanitizeInput(input: string): string {
    return input
      // 移除零宽字符
      .replace(/[\u200B-\u200D\u2060\uFEFF\u00AD]/g, '')
      // 规范化 Unicode
      .normalize('NFKC')
      // 移除控制字符（保留换行和制表符）
      .replace(/[\x00-\x08\v\f\x0E-\x1F\x7F]/g, '')
      // 限制连续空白
      .replace(/ {3,}/g, '  ')
      .trim()
  }
}

/**
 * 创建权限控制器
 */
export function createPermissionController(tappInstance: TappInstance): TappPermissionController {
  return new TappPermissionController(tappInstance)
}
