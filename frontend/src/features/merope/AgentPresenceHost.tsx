import { useEffect } from 'react'
import { useLocation } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'

/** 人设在场不跟面板引擎走：Gate 通过就挂，inbound 动态 import，不进首页静态图。 */
export function AgentPresenceHost() {
  const { isAuthenticated } = useAuth()
  const location = useLocation()

  useEffect(() => {
    if (!isAuthenticated) return
    let stop = () => {}
    let cancelled = false
    void import('./perception/inbound').then(({ startPresenceInbound }) => {
      if (cancelled) return
      stop = startPresenceInbound()
    })
    return () => {
      cancelled = true
      stop()
    }
  }, [isAuthenticated])

  useEffect(() => {
    let stop = () => {}
    let cancelled = false
    void import('./motion/playbackDirectionHost').then(
      ({ playbackDirection, retainPlaybackDirection }) => {
        if (cancelled) return
        if (isAuthenticated) stop = retainPlaybackDirection()
        else playbackDirection.stop()
      },
    )
    return () => {
      cancelled = true
      stop()
    }
  }, [isAuthenticated])

  useEffect(() => {
    let cancelled = false
    void import('./perception/inbound').then(({ notePresenceRoute }) => {
      if (!cancelled) notePresenceRoute(location.pathname)
    })
    return () => {
      cancelled = true
    }
  }, [location.pathname])

  return null
}
