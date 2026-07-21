import type { User } from '../../contexts/AuthContext'
import React, { memo, useCallback, useEffect, useRef, useState } from 'react'

import { createPortal } from 'react-dom'
import { API_URL } from '../../config'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { getCSRFToken } from '../../utils/csrf'
import { clearPlaylistCache } from '../../utils/musicPlayer'
import {
  clearAllUserCache,
  invalidateUserInfoCache,
} from '../../utils/userInfoCache'
import LoginForm from '../LoginForm'
import { UserModal } from './UserModal'

interface UserInfo {
  name: string
  avatar: string
  bio: string
  platform: string
}

interface UserSectionProps {
  onClosePanel: () => void
  /** 面板内路由跳转：收起 GCP 并替换历史哨兵（见 GlobalControlPanel.handleNavigateFromPanel） */
  onNavigateFromPanel: (path: string) => void
}

/**
 * 用户区域组件
 * 包含顶部用户信息按钮和用户弹窗逻辑
 *
 * memo：宿主 GlobalControlPanel 因音乐进度/歌词轮播频繁重渲染，
 * 本组件仅依赖稳定的 onClosePanel / onNavigateFromPanel 回调，隔离后不再跟随重渲染
 */
export const UserSection: React.FC<UserSectionProps> = memo(
  ({ onClosePanel, onNavigateFromPanel }) => {
    const {
      isAuthenticated: authIsAuthenticated,
      user: authUser,
      checkAuth,
    } = useAuth()
    const { t } = useI18n()
    const [user, setUser] = useState<User | null>(null)
    const [isAuthenticated, setIsAuthenticated] = useState(false)
    const [userInfo, setUserInfo] = useState<UserInfo | null>(null)

    // 弹窗状态机：'closed' -> 'mounting' -> 'visible' -> 'closing' -> 'closed'
    const [modalState, setModalState] = useState<
      'closed' | 'mounting' | 'visible' | 'closing'
    >('closed')

    // 处理弹窗状态机转换
    useEffect(() => {
      if (modalState === 'mounting') {
        // mounting 阶段：DOM 已渲染但不可见，等待布局稳定后进入 visible
        const timer = setTimeout(() => {
          setModalState('visible')
        }, 16) // 一帧时间
        return () => clearTimeout(timer)
      }

      if (modalState === 'closing') {
        // closing 阶段：播放关闭动画后进入 closed
        const timer = setTimeout(() => {
          setModalState('closed')
        }, 300)
        return () => clearTimeout(timer)
      }
    }, [modalState])

    // 滚动锁定 - 保存并恢复原始 overflow 值
    useEffect(() => {
      if (modalState !== 'closed') {
        const originalOverflow = document.body.style.overflow
        document.body.style.overflow = 'hidden'
        return () => {
          document.body.style.overflow = originalOverflow
        }
      }
    }, [modalState])

    /**
     * 切换 OAuth 画像源后，在本弹窗生命周期内优先用 /api/auth/me，
     * 避免管理员仍被 /api/profile/user-info 站长资料盖掉头像。
     */
    const preferSessionProfileRef = useRef(false)

    const applySessionUserInfo = useCallback(
      (session: {
        display_name?: string
        username?: string
        avatar_url?: string
        bio?: string
        auth_provider?: string
      }) => {
        const displayName =
          session.display_name ||
          session.username ||
          t.userModal.unknownUser
        const avatar =
          session.avatar_url ||
          `https://ui-avatars.com/api/?name=${encodeURIComponent(displayName)}&background=random`
        const bio = session.bio || t.userModal.defaultBio
        const platform =
          session.auth_provider === 'github'
            ? 'GitHub'
            : session.auth_provider === 'local'
              ? 'Local'
              : session.auth_provider || t.userModal.unknownPlatform
        setUserInfo({
          name: displayName,
          avatar,
          bio,
          platform,
        })
      },
      [t],
    )

    // 获取用户信息
    // 对于管理员：默认站长资料；切画像源后或 forceSession 时用登录会话（/api/auth/me）
    // 对于普通用户：使用 authUser / 会话
    const fetchUserInfo = useCallback(
      async (opts?: { forceSession?: boolean }) => {
        if (!authUser) return

        const forceSession =
          opts?.forceSession === true || preferSessionProfileRef.current

        if (forceSession) {
          try {
            const meRes = await fetch(`${API_URL}/api/auth/me`, {
              credentials: 'include',
            })
            if (meRes.ok) {
              const me = await meRes.json()
              if (me?.authenticated !== false && (me?.id || me?.username)) {
                applySessionUserInfo(me)
                return
              }
            }
          } catch {
            /* fall through to authUser */
          }
          applySessionUserInfo(authUser)
          return
        }

        // 管理员默认站长资料（公开站点形象）
        if (authUser.is_admin) {
          try {
            const profileResponse = await fetch(
              `${API_URL}/api/profile/user-info`,
            )
            if (profileResponse.ok) {
              const profileData = await profileResponse.json()
              if (profileData.success && profileData.user_info) {
                setUserInfo({
                  name: profileData.user_info.name || t.userModal.unknownUser,
                  avatar: profileData.user_info.avatar || '',
                  bio: profileData.user_info.bio || t.userModal.defaultBio,
                  platform:
                    profileData.user_info.platform ||
                    t.userModal.unknownPlatform,
                })
                return
              }
            }
          } catch (_error) {
            console.debug(
              '[UserSection] Failed to fetch admin profile, using authUser info',
            )
          }
        }

        applySessionUserInfo(authUser)
      },
      [authUser, t, applySessionUserInfo],
    )

    // 同步 AuthContext 的用户信息
    useEffect(() => {
      setIsAuthenticated(authIsAuthenticated)
      setUser(authUser as User | null)
      if (authUser) {
        void fetchUserInfo()
      }
    }, [authIsAuthenticated, authUser, fetchUserInfo])

    // 打开弹窗
    const openModal = useCallback(() => {
      if (modalState === 'closed') {
        setModalState('mounting')
      }
    }, [modalState])

    // 关闭弹窗
    const closeModal = useCallback(() => {
      if (modalState === 'visible' || modalState === 'mounting') {
        setModalState('closing')
      }
    }, [modalState])

    // 弹窗关闭后恢复管理员默认站长资料策略
    useEffect(() => {
      if (modalState === 'closed') {
        preferSessionProfileRef.current = false
      }
    }, [modalState])

    // 监听登录成功事件
    useEffect(() => {
      const handleLoginSuccess = () => {
        closeModal()
        invalidateUserInfoCache()
        getCSRFToken(true).catch(console.warn)
      }

      window.addEventListener('auth-login-success', handleLoginSuccess)
      return () => {
        window.removeEventListener('auth-login-success', handleLoginSuccess)
      }
    }, [closeModal])

    // 监听打开用户弹窗事件（从其他组件触发）
    useEffect(() => {
      const handleOpenUserModal = () => {
        openModal()
      }

      window.addEventListener('open-user-modal', handleOpenUserModal)
      return () => {
        window.removeEventListener('open-user-modal', handleOpenUserModal)
      }
    }, [openModal])

    // 处理用户信息区域点击
    const handleUserInfoClick = () => {
      openModal()
    }

    // 处理退出登录
    const handleLogout = useCallback(async () => {
      onClosePanel()

      // 触发认证状态变化事件
      window.dispatchEvent(
        new CustomEvent('auth-state-changed', {
          detail: {
            isAuthenticated: false,
            isAdmin: false,
          },
        }),
      )

      try {
        // 先清理用户临时安装的 Tapp（服务层按需加载，登出是低频路径）
        const { cleanupTemporaryTapps } = await import(
          '../../tapp/services/TappApiService',
        )
        await cleanupTemporaryTapps()
      } catch (error) {
        // 静默处理清理错误
        console.warn('[UserSection] Failed to cleanup temporary tapps:', error)
      }

      try {
        await fetch('/api/auth/logout', {
          method: 'POST',
          credentials: 'include',
        })
      } catch (_error) {
        // 静默处理退出错误
      }

      // 清除用户信息缓存
      clearAllUserCache()

      // 彻底清理所有本地状态和存储
      localStorage.clear()
      sessionStorage.clear()

      // 清空音乐播放器缓存
      clearPlaylistCache()

      // 手动删除所有Cookie
      document.cookie.split(';').forEach((cookie) => {
        const name = cookie.split('=')[0].trim()
        document.cookie = `${name}=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Strict`
        document.cookie = `${name}=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT`
      })

      window.location.href = '/login'
    }, [onClosePanel])

    return (
      <>
        {/* 头部 - 用户信息按钮 */}
        <button
          onClick={handleUserInfoClick}
          className="user-info-button flex items-center gap-3"
        >
          {isAuthenticated && userInfo ? (
            <>
              <img
                src={userInfo.avatar}
                alt={userInfo.name}
                className="w-10 h-10 rounded-full object-cover shrink-0"
                onError={(e) => {
                  e.currentTarget.src = `https://ui-avatars.com/api/?name=${encodeURIComponent(userInfo.name)}`
                }}
              />
              <div className="min-w-0">
                <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
                  {userInfo.name}
                </h3>
                <p className="text-xs text-gray-500 dark:text-gray-400 truncate">
                  {userInfo.bio.length > 30
                    ? `${userInfo.bio.substring(0, 30)}...`
                    : userInfo.bio}
                </p>
              </div>
            </>
          ) : (
            <>
              <svg
                className="w-5 h-5 text-gray-400 shrink-0"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"
                />
              </svg>
              <span className="text-sm text-gray-600 dark:text-gray-300 whitespace-nowrap">
                {t.userModal.pleaseLogin}
              </span>
            </>
          )}
        </button>

        {/* 用户信息/登录弹窗 - 使用 Portal 渲染到 body */}
        {modalState !== 'closed' &&
          createPortal(
            <>
              <div
                className={`user-modal-overlay ${modalState === 'visible' ? 'animate-in' : ''} ${modalState === 'closing' ? 'closing' : ''}`}
                onClick={closeModal}
              />
              {isAuthenticated && user && userInfo ? (
                <UserModal
                  user={user}
                  userInfo={userInfo}
                  isClosing={modalState === 'closing'}
                  canAnimate={modalState === 'visible'}
                  onClose={closeModal}
                  onLogout={handleLogout}
                  onNavigateFromPanel={onNavigateFromPanel}
                  onProfileApplied={() => {
                    preferSessionProfileRef.current = true
                    void (async () => {
                      await checkAuth()
                      // 强制 /me，避免管理员站长资料盖过刚选的 OAuth 画像
                      await fetchUserInfo({ forceSession: true })
                    })()
                  }}
                />
              ) : (
                <div
                  className={`user-modal-login-only ${modalState === 'visible' ? 'animate-in' : ''} ${modalState === 'closing' ? 'closing' : ''}`}
                >
                  <button
                    onClick={closeModal}
                    className="login-close-btn"
                    aria-label={t.common.close}
                  >
                    <svg
                      className="w-5 h-5"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M6 18L18 6M6 6l12 12"
                      />
                    </svg>
                  </button>
                  <LoginForm />
                </div>
              )}
            </>,
            document.body,
          )}
      </>
    )
  },
)

UserSection.displayName = 'UserSection'

// 导出用户和用户信息类型供外部使用
export type { UserInfo }

export default UserSection
