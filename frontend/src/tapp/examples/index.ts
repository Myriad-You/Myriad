/**
 * Tapp 示例应用集合
 * 内置教学 / 能力演示示例；更多应用见远程商店。
 *
 * 更新日期：2026-07
 *
 * 内置：helloWorld。Aro / 斗地主 / Three 实验室等完整应用见官方 tapp-store。
 */

// 导出类型
// 导入用于聚合
import type { ExampleTapp } from './tapps/types'
import { helloWorldTapp } from './tapps/helloWorld'

// 导出示例
export { helloWorldTapp } from './tapps/helloWorld'

export type { ExampleTapp } from './tapps/types'

/**
 * 内置示例 Tapp
 * 其他应用请从远程商店安装
 */
export const EXAMPLE_TAPPS: ExampleTapp[] = [
  helloWorldTapp,
]

export default EXAMPLE_TAPPS
