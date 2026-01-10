import React, { Suspense } from 'react'

/**
 * 通用图表懒加载封装：仅在渲染时加载 chart.js + react-chartjs-2
 */
export const LazyChart = React.lazy(async () => {
  const [{ Chart }, chartjs2] = await Promise.all([
    import('chart.js'),
    import('react-chartjs-2'),
  ])
  // 可在此处进行必要的 Chart 注册（若项目使用自动注册可省略）
  return { default: chartjs2.Line } // 示例：按需导出 Line 组件；其他图表可各自封装
})

export function ChartFallback({ height = 160 }: { height?: number }) {
  return <div style={{ height }} className="w-full animate-pulse bg-gray-200/40 dark:bg-white/5 rounded" />
}

export function WithLazyChart(props: React.ComponentProps<any>) {
  return (
    <Suspense fallback={<ChartFallback />}>
      <LazyChart {...props} />
    </Suspense>
  )
}
