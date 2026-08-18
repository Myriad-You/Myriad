import type { SVGProps } from 'react'

/**
 * Arael 生命开关打开时的标记：生命游戏的滑翔机
 * （Conway's Game of Life, glider）。
 *
 * 3×3 网格里的五格，是这个细胞自动机里最小的、会自己往前走的活物。
 * 换掉了原来的 `✦` / LuSparkles（每个产品都在用的「AI 生成」通用符号）。
 *
 * 纯方块而不是描边字形：缩到 14px 仍是五个点。方向是标准滑翔机朝右下走。
 * 纯 svg，走 currentColor。
 */
const CELL = 5.4
const R = 1.7
/** 3×3 网格：格距 7，整体 19.4，居中留边 2.3 */
const GRID = [2.3, 9.3, 16.3]
/** 滑翔机的五格：(列, 行) */
const GLIDER: Array<[number, number]> = [
  [1, 0],
  [2, 1],
  [0, 2],
  [1, 2],
  [2, 2],
]

export default function LifeMark(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden
      {...props}
    >
      {GLIDER.map(([col, row]) => (
        <rect
          key={`${col}-${row}`}
          x={GRID[col]}
          y={GRID[row]}
          width={CELL}
          height={CELL}
          rx={R}
          fill="currentColor"
        />
      ))}
    </svg>
  )
}
