import { useEffect } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { useI18n } from '../contexts/I18nContext'
import { ApiError, apiService } from '../services/api'
import { showError } from '../utils/toastManager'

// 初始化完成后不会在会话内回退；确认一次即可，失败才在下次导航重试。
let setupConfirmed = false

export function useSystemSetupCheck() {
  const { t } = useI18n()
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    if (setupConfirmed) return
    // ServeDir 会把 /setup 307 到 /setup/；先归一尾斜线再比较，否则会无限重载。
    if (location.pathname.replaceAll(/\/+$/g, '') === '/setup') return

    async function checkSetup() {
      try {
        const data = await apiService.get<{ is_setup_required?: boolean }>('/setup/status')
        if (!data.is_setup_required) {
          setupConfirmed = true
          return
        }
        console.warn('System not setup, redirecting to setup wizard...')

        // 硬跳转避开初始化期路由/动画抢跑。目标用规范 '/setup'（dev 下 '/setup/' 会 404）。
        window.location.replace('/setup')
      } catch (error) {
        // The backend answered: nothing to redirect on. Only an unreachable backend is worth a toast.
        if (error instanceof ApiError && error.status > 0) return
        console.error('Failed to check setup status:', error)
        showError(t.errors.setupCheckFailed)
      }
    }

    checkSetup()
  }, [location.pathname, navigate, t.errors.setupCheckFailed])
}
