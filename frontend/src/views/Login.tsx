/**
 * 登录视图组件
 */

import AnimatedView from '../components/AnimatedView'
import LoginForm from '../components/LoginForm'
import { useLoginScheduler } from '../hooks/animation/pages/simple'

export default function Login() {
  // 🆕 初始化页面级调度器
  useLoginScheduler()

  return (
    <AnimatedView className="min-h-screen flex items-center justify-center px-4 pt-20">
      <LoginForm />
    </AnimatedView>
  )
}
