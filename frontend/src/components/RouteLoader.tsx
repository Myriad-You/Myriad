import { useEffect, useState } from 'react'
import { useLocation } from 'react-router-dom'
import './RouteLoader.css'

export default function RouteLoader() {
  const [loading, setLoading] = useState(false)
  const [visible, setVisible] = useState(false)
  const location = useLocation()

  useEffect(() => {
    setLoading(true)
    setVisible(true)

    let hideTimer: ReturnType<typeof setTimeout> | null = null
    const timer = setTimeout(() => {
      setLoading(false)
      hideTimer = setTimeout(setVisible, 400, false)
    }, 600)

    return () => {
      clearTimeout(timer)
      if (hideTimer !== null) clearTimeout(hideTimer)
    }
  }, [location.pathname])

  if (!visible) return null

  return (
    <div className="route-loader-indicator">
      <div
        className={`route-loader-light ${loading ? 'entering' : 'exiting'}`}
      />
    </div>
  )
}
