/**
 * 路由配置
 * 定义所有应用路由及其对应的组件
 */

export interface RouteConfig {
  path: string
  component: () => Promise<{ default: React.ComponentType<any> }>
  title: string
  description?: string
  requiresAuth?: boolean
  requiresAdmin?: boolean
}

export const routes: RouteConfig[] = [
  {
    path: '/',
    component: () => import('../views/Home.tsx'),
    title: 'Myriad - 数字自我发现',
    description: '一键聚合你的多平台数据，生成AI个人分析报告',
  },
  {
    path: '/library',
    component: () => import('../views/Library.tsx'),
    title: '资料库 - Myriad',
    description: '浏览你的多平台数据收藏',
  },
  {
    path: '/brew',
    component: () => import('../views/Brew.tsx'),
    title: 'Brew 阅读 - Myriad',
    description: 'RSS/Atom 订阅阅读器',
    requiresAuth: true,
  },
  {
    path: '/reports',
    component: () => import('../views/Reports.tsx'),
    title: '数据报告 - Myriad',
    description: '基于5W框架的双层智能分析报告',
  },
  {
    path: '/config',
    component: () => import('../views/Config.tsx'),
    title: '系统配置 - Myriad',
    requiresAuth: true,
    requiresAdmin: true,
  },
  {
    path: '/data-management',
    component: () => import('../views/DataManagement.tsx'),
    title: '数据管理 - Myriad',
    requiresAuth: true,
    requiresAdmin: true,
  },
  {
    path: '/login',
    component: () => import('../views/Login.tsx'),
    title: '登录 - Myriad',
  },
  {
    path: '/setup',
    component: () => import('../views/Setup.tsx'),
    title: '初始化设置 - Myriad',
  },
  // Tapp 路由
  {
    path: '/tapp',
    component: () => import('../tapp/pages/TappListPage.tsx'),
    title: 'Tapp 应用 - Myriad',
    description: '管理和运行扩展应用',
  },
  {
    path: '/tapp/run',
    component: () => import('../views/TappRunView.tsx'),
    title: 'Tapp 多任务 - Myriad',
    description: '多窗口应用管理',
  },
  {
    path: '/tapp/run/:id',
    component: () => import('../views/TappRunView.tsx'),
    title: 'Tapp - Myriad',
  },
  {
    path: '/tapp/detail/:id',
    component: () => import('../views/TappDetailView.tsx'),
    title: 'Tapp 详情 - Myriad',
  },
]

/**
 * 根据路径查找路由配置
 * 支持带参数的路由（如 /tapp/run/:id）
 */
export function findRoute(path: string): RouteConfig | undefined {
  // 先尝试精确匹配
  const exactMatch = routes.find(route => route.path === path)
  if (exactMatch)
    return exactMatch

  // 再尝试模式匹配
  for (const route of routes) {
    if (route.path.includes(':')) {
      // 转换路由模式为正则
      const pattern = route.path.replace(/:\w+/g, '[^/]+')
      const regex = new RegExp(`^${pattern}$`)
      if (regex.test(path)) {
        return route
      }
    }
  }

  return undefined
}

/**
 * 路由动画配置
 */
export const routeAnimations = {
  initial: { opacity: 0, x: 20 },
  animate: { opacity: 1, x: 0 },
  exit: { opacity: 0, x: -20 },
  transition: { duration: 0.3, ease: 'easeInOut' },
}
