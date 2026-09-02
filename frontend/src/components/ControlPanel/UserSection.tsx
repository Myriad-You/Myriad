import type { User } from '../../contexts/AuthContext'
import { LuX } from '@lib/icons'

import React, { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import { useLocation } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { setForegroundSurface } from '../../features/merope/perception/surface'
import { useSiteOwnerProfile } from '../../hooks/useSiteOwnerProfile'
import { onProfileDisplayChanged } from '../../services/avatarSourceApi'
import { getCSRFToken } from '../../utils/csrf'
import { clearPlaylistCache } from '../../utils/musicPlayer'
import { lockScroll } from '../../utils/scrollLock'
import { clearAllUserCache } from '../../utils/userInfoCache'
import { Avatar } from '../Avatar'
import LoginForm from '../LoginForm'
import { UserModal } from './UserModal'

interface UserInfo {
  name: string
  /** 可能为空：<Avatar> 负责本地兜底 */
  avatar: string | null
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
    const location = useLocation()
    const [user, setUser] = useState<User | null>(null)
    const [isAuthenticated, setIsAuthenticated] = useState(false)

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

    // 滚动锁定：必须锁 html —— 本站 html 带 overflow，滚动容器是它不是 body
    useEffect(() => {
      if (modalState !== 'closed') {
        return lockScroll()
      }
    }, [modalState])

    // 站长的展示名/简介沿用平台画像（站点形象），与首页信息条同一份数据；
    // 普通用户没有平台资料，不必为此多打一次公开接口。
    const { profile: ownerProfile } = useSiteOwnerProfile({
      enabled: authUser?.is_owner === true,
    })

    /**
     * 头像一律取会话身份自己的（`/api/auth/me` 读的是画像源快照，与首页同源），
     * 名称/简介对站长优先用平台画像。
     *
     * 头像不再走 `/api/profile/user-info` 分支：来源已由用户显式选定并落成快照，
     * 前端不需要再分支，也不需要切换画像源后临时翻转策略的那套 ref。
     */
    const userInfo: UserInfo | null = useMemo(() => {
      if (!authUser) return null
      const sessionName =
        authUser.display_name || authUser.username || t.userModal.unknownUser
      return {
        name: ownerProfile?.name || sessionName,
        avatar: authUser.avatar_url ?? null,
        bio: ownerProfile?.bio || authUser.bio || t.userModal.defaultBio,
        platform:
          ownerProfile?.platform ||
          (authUser.auth_provider === 'github'
            ? 'GitHub'
            : authUser.auth_provider === 'local'
              ? 'Local'
              : authUser.auth_provider || t.userModal.unknownPlatform),
      }
    }, [authUser, ownerProfile, t])

    // 同步 AuthContext 的用户信息
    useEffect(() => {
      setIsAuthenticated(authIsAuthenticated)
      setUser(authUser as User | null)
    }, [authIsAuthenticated, authUser])

    // 别处（含其它标签页）换了头像 / 名称简介来源 → 重新探一次会话。
    // 只听 profile-display-changed：notifyAvatarChanged 会双发 avatar + profile-display，
    // 若两边都 checkAuth 会跨标签页打两次 /auth/me。
    useEffect(() => {
      return onProfileDisplayChanged(() => void checkAuth())
    }, [checkAuth])

    // 打开弹窗。登录/注册页已经是同一套表单，再叠一层弹窗会双卡背景。
    const openModal = useCallback(() => {
      if (
        !authIsAuthenticated &&
        (location.pathname === '/login' || location.pathname === '/register')
      ) {
        onClosePanel()
        return
      }
      if (modalState === 'closed') {
        setModalState('mounting')
        setForegroundSurface('user_modal')
        void import('../../utils/analyticsEvents').then(
          ({ trackProductEvent, AnalyticsEvents }) => {
            trackProductEvent(AnalyticsEvents.USER_MODAL_OPEN, {
              throttleMs: 5000,
            })
          },
        )
      }
    }, [authIsAuthenticated, location.pathname, modalState, onClosePanel])

    // 关闭弹窗
    const closeModal = useCallback(() => {
      if (modalState === 'visible' || modalState === 'mounting') {
        setModalState('closing')
        setForegroundSurface('control_panel')
      }
    }, [modalState])

    // 监听登录成功事件
    useEffect(() => {
      const handleLoginSuccess = () => {
        closeModal()
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

      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.LOGOUT, { flush: true })
        },
      )

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
        // 站点策略：登出即删 / 或顺带清理未活跃用户的个人安装
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
              <Avatar
                src={userInfo.avatar}
                name={userInfo.name}
                className="w-10 h-10 rounded-full object-cover shrink-0"
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
                className={`user-modal-overlay surface-dialog-backdrop ${isAuthenticated ? '' : 'user-modal-overlay--plain'} ${modalState === 'visible' ? 'animate-in' : ''} ${modalState === 'closing' ? 'closing' : ''}`}
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
                  // 头像/文案来源已切换：重新探会话拿新快照（本弹窗内立即回显）
                  onProfileApplied={() => void checkAuth()}
                />
              ) : (
                <div
                  className={`user-modal-login-only glass surface-dialog ${modalState === 'visible' ? 'animate-in' : ''} ${modalState === 'closing' ? 'closing' : ''}`}
                  role="dialog"
                  aria-modal="true"
                  aria-labelledby="login-form-title"
                >
                  <button
                    type="button"
                    onClick={closeModal}
                    className="user-modal-chrome-hit user-modal-close-float"
                    aria-label={t.common.close}
                    title={t.common.close}
                  >
                    <LuX aria-hidden />
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
