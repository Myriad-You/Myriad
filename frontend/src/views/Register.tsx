/**
 * 注册视图组件（PR #4）
 */

import { useMemo } from 'react'
import AnimatedView from '../components/AnimatedView'
import RegisterForm from '../components/RegisterForm'
import { useI18n } from '../contexts/I18nContext'
import { useLoginScheduler } from '../hooks/animation'
import { usePageSeo } from '../hooks/usePageSeo'
import { buildPrivatePageSeo } from '../utils/modulePageSeo'

export default function Register() {
  // 复用登录页的调度器（同样是简易入场动画）
  useLoginScheduler()
  const { t } = useI18n()

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: t.auth.register || t.nav.login,
          path: '/register',
        }),
      [t],
    ),
  )

  return (
    <AnimatedView className="auth-page min-h-screen flex items-center justify-center px-4 pt-20">
      <div className="auth-card glass">
        <RegisterForm />
      </div>
    </AnimatedView>
  )
}
