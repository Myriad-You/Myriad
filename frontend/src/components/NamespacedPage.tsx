import type { ReactNode } from 'react'
import type { ShellNamespace } from '../i18n'
import { Suspense } from 'react'
import { I18nNamespace } from '../contexts/I18nContext'
import { DocumentReady } from './DocumentReady'

/** fallback null：PageLoader 与页面数据态已覆盖等待，不要再加路由级 spinner。 */
export function SuspensePage({ children }: { children: ReactNode }) {
  return (
    <Suspense fallback={null}>
      <DocumentReady>{children}</DocumentReady>
    </Suspense>
  )
}

export function NamespacedPage({
  names,
  children,
}: {
  names: readonly ShellNamespace[]
  children: ReactNode
}) {
  return (
    <I18nNamespace names={names}>
      <SuspensePage>{children}</SuspensePage>
    </I18nNamespace>
  )
}
