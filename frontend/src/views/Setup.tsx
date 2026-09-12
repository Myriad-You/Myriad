import { useMemo } from 'react'
import AnimatedView from '../components/AnimatedView'
import SetupWizard from '../components/SetupWizard'
import { useI18n } from '../contexts/I18nContext'
import { useSetupScheduler } from '../hooks/animation'
import { usePageSeo } from '../hooks/usePageSeo'
import { buildPrivatePageSeo } from '../utils/modulePageSeo'

export default function Setup() {
  useSetupScheduler()
  const { t } = useI18n()

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: t.nav.config,
          path: '/setup',
        }),
      [t],
    ),
  )

  return (
    <AnimatedView className="min-h-screen flex items-center justify-center px-4 pt-20 pb-16">
      <SetupWizard />
    </AnimatedView>
  )
}
