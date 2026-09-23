import { LuX } from '@lib/chromeStrokeIcons'
import React, {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useLocation } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { setForegroundSurface } from '../../features/merope/perception/surface'
import { useSiteOwnerProfile } from '../../hooks/useSiteOwnerProfile'
import { onProfileDisplayChanged } from '../../services/avatarSourceApi'
import { emitAppEvent } from '../../utils/appEvents'
import { getCSRFToken } from '../../utils/csrf'
import { clearPlaylistCache } from '../../utils/musicPlayer'
import { lockScroll } from '../../utils/scrollLock'
import { Avatar } from '../Avatar'
import './UserSection.css'

const loadUserModalEntrance = () => import('./UserModalEntrance')
const UserModalEntrance = lazy(loadUserModalEntrance)
const loadUserModal = () => import('./UserModal')
const UserModal = lazy(loadUserModal)
const loadLoginForm = () => import('../LoginForm')
const LoginForm = lazy(loadLoginForm)

interface UserInfo {
  name: string
  avatar: string | null
  bio: string
  platform: string
}

interface UserSectionProps {
  onClosePanel: () => void
  // 面板内跳转：收起 GCP 并替换历史哨兵。
  onNavigateFromPanel: (path: string) => void
}

export const UserSection: React.FC<UserSectionProps> = memo(
  ({ onClosePanel, onNavigateFromPanel }) => {
    const { isAuthenticated, user, checkAuth } = useAuth()
    const { t } = useI18n()
    const location = useLocation()

    const [modalState, setModalState] = useState<
      'closed' | 'mounting' | 'visible' | 'closing'
    >('closed')

    useEffect(() => {
      if (modalState === 'closing') {
        const timer = setTimeout(() => {
          setModalState('closed')
        }, 300)
        return () => clearTimeout(timer)
      }
    }, [modalState])

    const onModalReady = useCallback(() => {
      setModalState(state => state === 'mounting' ? 'visible' : state)
    }, [])
    const prepareUserModal = useCallback(() => {
      const loading = isAuthenticated ? loadUserModal() : loadLoginForm()
      void Promise.all([loadUserModalEntrance(), loading]).catch(() => {})
    }, [isAuthenticated])

    // 必须锁 html：本站滚动容器是 html 不是 body。
    useEffect(() => {
      if (modalState !== 'closed') {
        return lockScroll()
      }
    }, [modalState])

    const { profile: ownerProfile } = useSiteOwnerProfile({
      enabled: user?.is_owner === true,
    })

    const userInfo: UserInfo | null = useMemo(() => {
      if (!user) return null
      const sessionName =
        user.display_name || user.username || t.userModal.unknownUser
      return {
        name: ownerProfile?.name || sessionName,
        avatar: user.avatar_url ?? null,
        bio: ownerProfile?.bio || user.bio || t.userModal.defaultBio,
        platform:
          ownerProfile?.platform ||
          (user.auth_provider === 'github'
            ? 'GitHub'
            : user.auth_provider === 'local'
              ? 'Local'
              : user.auth_provider || t.userModal.unknownPlatform),
      }
    }, [user, ownerProfile, t])

    useEffect(() => {
      return onProfileDisplayChanged(() => void checkAuth())
    }, [checkAuth])

    const openModal = useCallback(() => {
      if (
        !isAuthenticated &&
        (location.pathname === '/login' || location.pathname === '/register')
      ) {
        onClosePanel()
        return
      }
      if (modalState === 'closed') {
        prepareUserModal()
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
    }, [isAuthenticated, location.pathname, modalState, onClosePanel, prepareUserModal])

    const closeModal = useCallback(() => {
      if (modalState === 'visible' || modalState === 'mounting') {
        setModalState('closing')
        setForegroundSurface('control_panel')
      }
    }, [modalState])

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

    useEffect(() => {
      const handleOpenUserModal = () => {
        openModal()
      }

      window.addEventListener('open-user-modal', handleOpenUserModal)
      return () => {
        window.removeEventListener('open-user-modal', handleOpenUserModal)
      }
    }, [openModal])

    const handleLogout = useCallback(async () => {
      onClosePanel()

      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.LOGOUT, { flush: true })
        },
      )

      emitAppEvent('auth-state-changed', {
            isAuthenticated: false,
            isAdmin: false,
          })

      try {
        const { cleanupTemporaryTapps } = await import(
          '../../tapp/services/TappApiService',
        )
        await cleanupTemporaryTapps()
      } catch (error) {
        console.warn('[UserSection] Failed to cleanup temporary tapps:', error)
      }

      try {
        await fetch('/api/auth/logout', {
          method: 'POST',
          credentials: 'include',
        })
      } catch {
      }

      localStorage.clear()
      sessionStorage.clear()

      clearPlaylistCache()

      document.cookie.split(';').forEach((cookie) => {
        const name = cookie.split('=')[0].trim()
        document.cookie = `${name}=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Strict`
        document.cookie = `${name}=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT`
      })

      window.location.href = '/login'
    }, [onClosePanel])

    return (
      <>
        <button
          type="button"
          onClick={openModal}
          onPointerEnter={prepareUserModal}
          onFocus={prepareUserModal}
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
                    ? `${userInfo.bio.slice(0, 30)}...`
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

        {modalState !== 'closed' &&
          createPortal(
            <Suspense fallback={null}>
              <UserModalEntrance onReady={onModalReady}>
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
              </UserModalEntrance>
            </Suspense>,
            document.body,
          )}
      </>
    )
  },
)

UserSection.displayName = 'UserSection'

export type { UserInfo }

export default UserSection
