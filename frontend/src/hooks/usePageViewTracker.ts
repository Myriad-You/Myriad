import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router-dom'
import { useAuth } from '../contexts/AuthContext'

/** SPA route → pageview + engagement。等 auth；排除管理员/站长自己。 */
export function usePageViewTracker() {
  const location = useLocation()
  const { isAdmin, hasChecked, user } = useAuth()
  const isOwner = Boolean(user?.is_owner)
  const isStaff = isAdmin || isOwner
  const lastPathRef = useRef<string | null>(null)

  useEffect(() => {
    void import('../utils/siteAnalytics').then((m) => {
      m.setAnalyticsStaffSession({ isAdmin, isOwner })
    })
    void import('../utils/googleAnalytics').then((m) => {
      m.setGoogleAnalyticsStaffExcluded(isStaff)
    })
    void import('../utils/umamiAnalytics').then((m) => {
      m.setUmamiStaffExcluded(isStaff)
    })
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
    void import('../utils/siteAnalytics').then((m) => {
      m.trackPageview(path)
    })
    void import('../utils/googleAnalytics').then((m) => {
      m.trackGooglePageview(path)
    })
    void import('../utils/umamiAnalytics').then((m) => {
      m.trackUmamiPageview(path)
    })
  }, [location.pathname, hasChecked, isStaff])
}
