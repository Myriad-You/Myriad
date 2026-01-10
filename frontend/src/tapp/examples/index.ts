/**
 * Tapp 示例应用集合
 * 仅包含 Hello World 教学示例
 * 其他应用已迁移到远程商店：https://github.com/Myriad-You/tapp-store
 *
 * 更新日期：2025-12
 */

// 导出类型
// 导入用于聚合
import type { ExampleTapp } from './tapps/types'
import { helloWorldTapp } from './tapps/helloWorld'

// 导出 Hello World 示例（用于教学演示）
export { helloWorldTapp } from './tapps/helloWorld'

export type { ExampleTapp } from './tapps/types'

/**
 * 内置示例 Tapp（仅 Hello World）
 * 其他应用请从远程商店安装
 */
export const EXAMPLE_TAPPS: ExampleTapp[] = [
  helloWorldTapp,
]

/**
 * 按分类获取示例
 */
export function getExamplesByCategory(category: ExampleTapp['category']): ExampleTapp[] {
  return EXAMPLE_TAPPS.filter(t => t.category === category)
}

/**
 * 分类名称映射（本地 + 远程）
 */
export const CATEGORY_NAMES: Record<string, string> = {
  // 本地分类
  widget: '小组件',
  tool: '工具',
  tools: '工具',
  platform: '平台',
  demo: '演示',
  test: '测试',
  // 远程商店分类
  productivity: '效率工具',
  games: '游戏',
  game: '游戏',
  entertainment: '娱乐',
  social: '社交',
  utilities: '实用工具',
  development: '开发',
  media: '媒体',
  ai: '人工智能',
  music: '音乐',
  visualization: '可视化',
  data: '数据',
}

/**
 * 获取分类显示名称
 */
export function getCategoryName(categoryId: string): string {
  return CATEGORY_NAMES[categoryId.toLowerCase()] || categoryId
}

/**
 * 获取所有分类
 */
export function getAllCategories(): { id: string, name: string, count: number }[] {
  const categories: Record<string, number> = {}
  EXAMPLE_TAPPS.forEach((t) => {
    categories[t.category] = (categories[t.category] || 0) + 1
  })

  return Object.entries(categories).map(([id, count]) => ({
    id,
    name: getCategoryName(id),
    count,
  }))
}

export default EXAMPLE_TAPPS
