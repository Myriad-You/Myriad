import { useEffect } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { showError } from '../utils/toastManager'

export function useSystemSetupCheck() {
  const { t } = useI18n()
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    // ServeDir 会把 /setup 307 到 /setup/；先归一尾斜线再比较，否则会无限重载。
    if (location.pathname.replaceAll(/\/+$/g, '') === '/setup') return

    async function checkSetup() {
      try {
        const response = await fetch(`${API_URL}/api/setup/status`)
        if (!response.ok) return

        const data = await response.json()
        if (data.is_setup_required) {
          console.warn('System not setup, redirecting to setup wizard...')

          // 硬跳转避开初始化期路由/动画抢跑。目标用规范 '/setup'（dev 下 '/setup/' 会 404）。
          window.location.replace('/setup')
        }
      } catch (error) {
        console.error('Failed to check setup status:', error)
        showError(t.errors.setupCheckFailed)
      }
    }

    checkSetup()
  }, [location.pathname, navigate, t.errors.setupCheckFailed])
}
