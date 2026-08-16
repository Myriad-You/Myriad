/**
 * 登录视图组件
 */

import { useMemo } from 'react'
import AnimatedView from '../components/AnimatedView'
import LoginForm from '../components/LoginForm'
import { useI18n } from '../contexts/I18nContext'
import { useLoginScheduler } from '../hooks/animation'
import { usePageSeo } from '../hooks/usePageSeo'
import { buildPrivatePageSeo } from '../utils/modulePageSeo'

export default function Login() {
  // 🆕 初始化页面级调度器
  useLoginScheduler()
  const { t } = useI18n()

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: t.auth.login || t.nav.login,
          path: '/login',
        }),
      [t],
    ),
  )

  return (
    <AnimatedView className="auth-page min-h-screen flex items-center justify-center px-4 pt-20">
      <div className="auth-card glass">
        <LoginForm />
      </div>
    </AnimatedView>
  )
}
