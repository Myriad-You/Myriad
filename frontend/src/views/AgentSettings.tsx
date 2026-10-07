import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import AgentSettingsForm from '../components/agent/settings/AgentSettingsForm'
import { AGENT_SETTINGS_PATH } from '../components/agent/settings/agentSettingsPath'
import AnimatedView from '../components/AnimatedView'
import { useAuth } from '../contexts/AuthContext'
import { useConfigI18n as useI18n } from '../contexts/I18nContext'
import { usePageSeo } from '../hooks/usePageSeo'
import { buildPrivatePageSeo } from '../utils/modulePageSeo'

export default function AgentSettings() {
  const navigate = useNavigate()
  const { t } = useI18n()
  const { isAdmin: authIsAdmin, isAuthenticated, hasChecked } = useAuth()
  const [loading, setLoading] = useState(true)
  const [isAdmin, setIsAdmin] = useState(false)

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: t.config.agent,
          path: AGENT_SETTINGS_PATH,
          description: t.config.agentDesc,
        }),
      [t],
    ),
  )

  useEffect(() => {
    if (!hasChecked) return
    if (!isAuthenticated) {
      navigate('/login', { replace: true })
      return
    }
    if (!authIsAdmin) {
      navigate('/', { replace: true })
      return
    }
    setIsAdmin(true)
    setLoading(false)
  }, [hasChecked, authIsAdmin, isAuthenticated, navigate])

  if (loading || !isAdmin) {
    return null
  }

  return (
    <AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
      <AgentSettingsForm />
    </AnimatedView>
  )
}
