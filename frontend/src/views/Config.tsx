import { useEffect, useMemo, useState } from 'react'

import { useNavigate } from 'react-router-dom'
import AnimatedView from '../components/AnimatedView'
import ConfigForm from '../components/ConfigForm'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useConfigScheduler } from '../hooks/animation'
import { usePageSeo } from '../hooks/usePageSeo'
import { buildPrivatePageSeo } from '../utils/modulePageSeo'
import { hasSessionHint } from '../utils/sessionDetection'

export default function Config() {
  useConfigScheduler()

  const navigate = useNavigate()
  const { t } = useI18n()
  const { isAdmin: authIsAdmin, isAuthenticated, checkAuth } = useAuth()
  const [loading, setLoading] = useState(true)
  const [isAdmin, setIsAdmin] = useState(false)

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: t.nav.config,
          path: '/config',
        }),
      [t],
    ),
  )

  useEffect(() => {
    if (!isAuthenticated) {
      if (hasSessionHint()) {
        checkAuth()
      } else {
        navigate('/login', { replace: true })
      }
    } else {
      if (!authIsAdmin) {
        navigate('/', { replace: true })
        return
      }
      setIsAdmin(true)
      setLoading(false)
    }
  }, [authIsAdmin, isAuthenticated, checkAuth, navigate])

  if (loading) {
    return null
  }

  if (!isAdmin) {
    return null
  }

  return (
    <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      <ConfigForm />
    </AnimatedView>
  )
}
