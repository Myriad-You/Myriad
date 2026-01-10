/**
 * 用户信息缓存工具
 * 用于缓存首页站长信息，避免频繁请求
 *
 * ⚠️ 安全说明：
 * - 只缓存非敏感的展示信息（头像、昵称、简介）
 * - 认证状态(is_admin)每次实时验证，不缓存
 * - 所有敏感操作仍需后端验证 Cookie + CSRF Token
 */

import { API_URL } from '../config'

// 缓存key - 仅用于展示信息（公开资料）
const USER_INFO_CACHE_KEY = 'myriad_profile_display_cache'
const USER_INFO_CACHE_TIME_KEY = 'myriad_profile_display_cache_time'

// 缓存过期时间
const PROFILE_CACHE_DURATION = 30 * 60 * 1000 // 站长展示信息缓存 30 分钟

export interface UserInfo {
  name: string
  avatar: string
  bio: string
  is_admin: boolean
  platform?: string
}

// 展示信息类型（不含权限，可安全缓存）
interface ProfileDisplayInfo {
  name?: string
  avatar?: string
  bio?: string
  platform?: string
}

/**
 * 获取缓存数据
 */
function getCachedData<T>(cacheKey: string, cacheTimeKey: string, maxAge: number): T | null {
  try {
    const cached = localStorage.getItem(cacheKey)
    const cacheTime = localStorage.getItem(cacheTimeKey)

    if (cached && cacheTime) {
      const cacheAge = Date.now() - Number.parseInt(cacheTime)
      if (cacheAge < maxAge) {
        return JSON.parse(cached) as T
      }
    }
  }
  catch (e) {
    console.warn('读取用户信息缓存失败:', e)
  }
  return null
}

/**
 * 设置缓存数据
 */
function setCachedData<T>(cacheKey: string, cacheTimeKey: string, data: T): void {
  try {
    localStorage.setItem(cacheKey, JSON.stringify(data))
    localStorage.setItem(cacheTimeKey, Date.now().toString())
  }
  catch (e) {
    console.warn('写入用户信息缓存失败:', e)
  }
}

/**
 * 清除指定缓存
 */
function clearCache(cacheKey: string, cacheTimeKey: string): void {
  try {
    localStorage.removeItem(cacheKey)
    localStorage.removeItem(cacheTimeKey)
  }
  catch (e) {
    // 忽略错误
  }
}

/**
 * 获取登录认证信息（实时验证，不缓存）
 * ⚠️ 安全：权限信息必须实时验证，防止本地篡改
 */
async function getAuthInfoRealtime(): Promise<{ isLoggedIn: boolean, is_admin: boolean, username?: string, display_name?: string }> {
  try {
    const response = await fetch(`${API_URL}/api/auth/me`, {
      credentials: 'include',
    })

    if (response.ok) {
      const data = await response.json()
      return {
        isLoggedIn: true,
        is_admin: data.is_admin || false,
        username: data.username,
        display_name: data.display_name,
      }
    }
  }
  catch {
    // 获取认证信息失败时静默处理
  }

  return { isLoggedIn: false, is_admin: false }
}

/**
 * 获取公开的站长资料（带缓存）
 * ✅ 安全：这是公开信息，可以缓存
 */
async function getProfileInfoWithCache(): Promise<ProfileDisplayInfo | null> {
  // 检查缓存
  const cached = getCachedData<ProfileDisplayInfo>(
    USER_INFO_CACHE_KEY,
    USER_INFO_CACHE_TIME_KEY,
    PROFILE_CACHE_DURATION,
  )

  if (cached !== null) {
    return cached
  }

  // 从后端获取
  try {
    const response = await fetch(`${API_URL}/api/profile/user-info`)
    if (response.ok) {
      const data = await response.json()
      if (data.success && data.user_info) {
        const result: ProfileDisplayInfo = {
          name: data.user_info.name,
          avatar: data.user_info.avatar,
          bio: data.user_info.bio,
          platform: data.user_info.platform,
        }
        setCachedData(USER_INFO_CACHE_KEY, USER_INFO_CACHE_TIME_KEY, result)
        return result
      }
    }
  }
  catch (e) {
    console.warn('获取站长资料失败:', e)
  }

  return null
}

/**
 * 获取用户头像（带缓存）
 * 轻量级方法，供需要快速获取头像的场景使用
 */
export async function getUserAvatarWithCache(fallbackUsername?: string): Promise<string> {
  const profileInfo = await getProfileInfoWithCache()

  if (profileInfo?.avatar) {
    return profileInfo.avatar
  }

  // 返回默认头像
  const name = fallbackUsername || 'User'
  return `https://ui-avatars.com/api/?name=${encodeURIComponent(name)}&background=random`
}

/**
 * 获取完整用户信息（合并认证状态和公开资料）
 * 这是首页信息条使用的主要方法
 *
 * @param skipAuthCheck - 如果为 true，跳过认证检查（当已从 AuthContext 获取认证状态时）
 * ⚠️ 认证状态实时验证，展示信息使用缓存
 */
export async function getUserInfoWithCache(skipAuthCheck: boolean = false): Promise<UserInfo> {
  // 默认访客信息
  let userInfo: UserInfo = {
    name: 'Myriad Dashboard',
    avatar: 'https://ui-avatars.com/api/?name=Myriad&background=random',
    bio: '欢迎访问我的个人仪表盘',
    is_admin: false,
  }

  // 1. 获取认证信息和公开资料
  let authInfo: { isLoggedIn: boolean, is_admin: boolean, username?: string, display_name?: string } = {
    isLoggedIn: false,
    is_admin: false,
  }
  let profileInfo: ProfileDisplayInfo | null = null

  if (skipAuthCheck) {
    // 只获取公开资料，跳过认证检查（避免重复请求）
    profileInfo = await getProfileInfoWithCache()
  }
  else {
    // 并行获取：认证信息(实时) + 公开资料(缓存)
    [authInfo, profileInfo] = await Promise.all([
      getAuthInfoRealtime(), // ⚠️ 实时验证权限
      getProfileInfoWithCache(), // ✅ 可缓存的展示信息
    ])
  }

  // 2. 如果已登录，使用账号基本信息
  if (authInfo.isLoggedIn) {
    userInfo = {
      name: authInfo.display_name || authInfo.username || userInfo.name,
      avatar: `https://ui-avatars.com/api/?name=${encodeURIComponent(authInfo.username || 'User')}&background=random`,
      bio: '这家伙很懒，没有介绍呢',
      is_admin: authInfo.is_admin, // ⚠️ 来自实时验证
    }
  }

  // 3. 如果有公开资料，优先使用（站长展示）
  if (profileInfo) {
    if (profileInfo.name)
      userInfo.name = profileInfo.name
    if (profileInfo.avatar)
      userInfo.avatar = profileInfo.avatar
    if (profileInfo.bio)
      userInfo.bio = profileInfo.bio
    if (profileInfo.platform)
      userInfo.platform = profileInfo.platform
  }

  return userInfo
}

/**
 * 强制刷新展示信息缓存
 * 在站长更新资料后调用
 */
export function invalidateUserInfoCache(): void {
  clearCache(USER_INFO_CACHE_KEY, USER_INFO_CACHE_TIME_KEY)
}

/**
 * 认证缓存清除（兼容性保留）
 * 认证状态不缓存，此方法保留用于 API 兼容
 */
export function invalidateAuthCache(): void {
  // 认证状态不缓存，无需操作
}

/**
 * 清除所有用户相关缓存
 */
export function clearAllUserCache(): void {
  invalidateUserInfoCache()
}

/**
 * 获取 CSRF Token（带内存缓存）
 * ✅ 安全：CSRF Token 存内存不存 localStorage，刷新即失效
 */
let csrfTokenCache: { token: string, timestamp: number } | null = null
const CSRF_CACHE_DURATION = 10 * 60 * 1000 // CSRF Token 缓存 10 分钟

export async function getCsrfTokenWithCache(): Promise<string> {
  // 检查内存缓存
  if (csrfTokenCache && (Date.now() - csrfTokenCache.timestamp < CSRF_CACHE_DURATION)) {
    return csrfTokenCache.token
  }

  try {
    const response = await fetch(`${API_URL}/api/csrf-token`, {
      credentials: 'include',
    })
    if (response.ok) {
      const data = await response.json()
      if (data.csrf_token) {
        csrfTokenCache = {
          token: data.csrf_token,
          timestamp: Date.now(),
        }
        return data.csrf_token
      }
    }
  }
  catch (e) {
    console.warn('获取 CSRF Token 失败:', e)
  }

  return ''
}

/**
 * 清除 CSRF Token 缓存
 */
export function invalidateCsrfCache(): void {
  csrfTokenCache = null
}
