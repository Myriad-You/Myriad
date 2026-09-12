import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router-dom'
import { useAuth } from '../contexts/AuthContext'
import {
  AnalyticsEvents,
  trackProductEvent,
} from '../utils/analyticsEvents'
import {
  setGoogleAnalyticsStaffExcluded,
  trackGooglePageview,
} from '../utils/googleAnalytics'
import {
  getOrCreateVisitorId,
  setAnalyticsStaffSession,
  trackEvent,
  trackPageview,
} from '../utils/siteAnalytics'
import {
  setUmamiStaffExcluded,
  trackUmamiPageview,
} from '../utils/umamiAnalytics'

export {
  AnalyticsEvents,
  getOrCreateVisitorId,
  setAnalyticsStaffSession,
  trackEvent,
  trackPageview,
  trackProductEvent,
}

/** SPA route → pageview + engagement。等 auth；排除管理员/站长自己。 */
export function usePageViewTracker() {
  const location = useLocation()
  const { isAdmin, hasChecked, user } = useAuth()
  const isOwner = Boolean(user?.is_owner)
  const isStaff = isAdmin || isOwner
  const lastPathRef = useRef<string | null>(null)

  useEffect(() => {
    setAnalyticsStaffSession({ isAdmin, isOwner })
    setGoogleAnalyticsStaffExcluded(isStaff)
    setUmamiStaffExcluded(isStaff)
  }, [isAdmin, isOwner, isStaff])

  useEffect(() => {
    if (!hasChecked) return
    if (isStaff) {
      lastPathRef.current = location.pathname || '/'
      return
    }

    const path = location.pathname || '/'
    if (lastPathRef.current === path) return
    lastPathRef.current = path
    trackPageview(path)

    // 第三方统计 SPA page_view（未配置时排队，配置后补发）。
    trackGooglePageview(path)
    trackUmamiPageview(path)
  }, [location.pathname, hasChecked, isStaff])
}

export default usePageViewTracker
