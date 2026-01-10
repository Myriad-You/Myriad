/**
 * 初始化设置视图组件
 */

import AnimatedView from '../components/AnimatedView'
import SetupWizard from '../components/SetupWizard'
import { useSetupScheduler } from '../hooks/animation/pages/simple'

export default function Setup() {
  // 🆕 初始化页面级调度器
  useSetupScheduler()

  return (
    <AnimatedView className="min-h-screen flex items-center justify-center px-4 pt-20">
      <SetupWizard />
    </AnimatedView>
  )
}
