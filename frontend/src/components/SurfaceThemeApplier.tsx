/**
 * 表面主题应用器 —— 全站级挂载点
 *
 * 作用：在 AppLayout 常驻挂载，触发 useWidgetTheme 的初始化，
 * 从后端读取保存的表面主题并写入 html[data-surface]，使**所有**页面
 * （含无小组件的报告页 / Tapp 页）的 .glass 组件都跟随主题。
 *
 * 另常驻注入流体玻璃的边缘折射 SVG 滤镜（#liquid-lens，0×0 不可见）。
 * theme.css 在 html.surface-lens[data-surface='liquid'] 下把它接进
 * backdrop-filter，复刻苹果 Liquid Glass 的材质解剖学，三路合成：
 * - **frost**：背后内容整体高斯模糊（stdDeviation 2 ≈ CSS blur(2px)，
 *   仅存一丝雾感，背后内容基本清晰）—— 磨砂内部；
 * - **bent rim**：背后内容按位移图向卡片中心弯折采样（凸透镜边缘的
 *   挤压光学感）再轻糊，环形蒙版只保留边缘一圈 —— 光学透亮的玻璃厚边；
 * - **specular**：从 frost 提取亮部、以顶部加权的环形蒙版裁出、
 *   screen 混合叠顶 —— 镜面高光取自背后内容本身，随内容/光源实时
 *   变化（静态画上去的高光在 lens 模式下由 theme.css 退场）。
 * 位移/合成都是逐像素一次读取，远廉于 blur 卷积。滤镜定义与消费方
 * 同文档常驻，避免 url(#) 悬空引用导致 Chromium 丢弃整条链。
 *
 * surface-lens 类按引擎能力打标：backdrop-filter 的 url() 引用仅
 * Chromium 系支持，Safari/Firefox 会整条静默丢弃且 @supports 测不出
 * （值在语法层面合法），故以 navigator.userAgentData（Chromium 独有）
 * 作为运行时信号；未打标浏览器停留在纯绘制层的流体观感。
 *
 * 主题变化引起的重渲染被隔离在此组件内，不波及应用树。
 */

import { useEffect } from 'react'

import { useWidgetTheme } from '../hooks/useWidgetTheme'

/* 位移图（单张双通道：R=横向 G=纵向，两条渐变 screen 混合叠加）：
   通道从边缘 255/0 经陡峭缓动坡道在 14% 处回到中性 127（中央平台零位移）。
   feDisplacementMap 语义：采样点 = 当前点 + scale × (通道/255 − 0.5)，
   四边一律向卡片中心偏采 —— 边缘呈现更靠中心的内容（凸透镜挤压），
   且永不越界采样（不会出现边缘拖影）。坡道在最外 5% 最陡，
   挤压集中成一条窄光学带，正是玻璃厚边的观感。
   性能约定（本文件所有贴图同此）：通道混合与位移场软化（模糊揉圆
   直角过渡、消坡道折痕）全部烘焙进 data URI 的 SVG 内部滤镜 ——
   贴图解码时栅格化一次、按 URL 全局缓存，滤镜运行链上零逐帧成本；
   烘焙后软化随贴图拉伸缩放，与坡道几何同比例，观感一致。
   注意内部滤镜必须声明 sRGB 插值，与外层滤镜链一致，否则通道值漂移 */
const LENS_STOPS: readonly (readonly [number, number])[] = [
  [0, 255],
  [0.02, 235],
  [0.05, 180],
  [0.08, 143],
  [0.11, 129],
  [0.14, 127],
  [0.86, 127],
  [0.89, 126],
  [0.92, 112],
  [0.95, 75],
  [0.98, 20],
  [1, 0],
]

function lensChannelGradient(channel: 'r' | 'g', id: string): string {
  const stops = LENS_STOPS.map(([offset, v]) => {
    const hex = v.toString(16).padStart(2, '0')
    const color = channel === 'r' ? `#${hex}0000` : `#00${hex}00`
    return `<stop offset="${offset}" stop-color="${color}"/>`
  }).join('')
  const axis = channel === 'r' ? 'x2="1" y2="0"' : 'x2="0" y2="1"'
  return `<linearGradient id="${id}" x1="0" y1="0" ${axis}>${stops}</linearGradient>`
}

const LENS_MAP = `data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256">`
  + `<defs>${
   lensChannelGradient('r', 'rx')
   }${lensChannelGradient('g', 'gy')
   }<filter id="s" x="-12%" y="-12%" width="124%" height="124%" color-interpolation-filters="sRGB">`
  + `<feGaussianBlur stdDeviation="10"/>`
  + `</filter>`
  + `</defs>`
  + `<g filter="url(#s)">`
  + `<rect width="256" height="256" fill="url(#rx)"/>`
  + `<rect width="256" height="256" fill="url(#gy)" style="mix-blend-mode:screen"/>`
  + `</g>`
  + `</svg>`,
)}`

/* 环形蒙版：evenodd 打洞，外框到内孔 36px（= 位移坡道的 14% 区），
   中心 alpha=0；高斯软化（烘焙）让弯折带与磨砂内部无缝过渡。
   内外框均为圆角矩形（外 r28 ≈ 11%，内 r20），与卡片圆角对齐，
   弯折带在角部沿圆弧走而非直角折 */
const RIM_OUTER
  = 'M28 0h200a28 28 0 0 1 28 28v200a28 28 0 0 1-28 28H28'
    + 'a28 28 0 0 1-28-28V28A28 28 0 0 1 28 0z'
const RIM_INNER
  = 'M56 36h144a20 20 0 0 1 20 20v144a20 20 0 0 1-20 20H56'
    + 'a20 20 0 0 1-20-20V56a20 20 0 0 1 20-20z'

function featherFilterDef(id: string, stdDeviation: number): string {
  return (
    `<filter id="${id}" x="-18%" y="-18%" width="136%" height="136%" color-interpolation-filters="sRGB">`
    + `<feGaussianBlur stdDeviation="${stdDeviation}"/>`
    + '</filter>'
  )
}

const LENS_RIM = `data:image/svg+xml,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256">'
  + `<defs>${featherFilterDef('f', 7)}</defs>`
  + `<path filter="url(#f)" fill-rule="evenodd" fill="#fff" d="${RIM_OUTER} ${RIM_INNER}"/>`
  + '</svg>',
)}`

/* 镜面蒙版：决定「背景亮部提取」贴在哪 —— 环形按垂直渐变加权
   （顶亮 0.95 · 侧收 0.3 · 底回亮 0.5，玻璃下缘的回光）；
   滤镜内高斯软化后与亮部相乘。刻意只做方向加权、不做定位热点
   （居中光叶之类）：每张卡同位置的静态高光会立刻穿帮，
   高光的「位置」必须来自背景亮部本身 */
const LENS_SPECULAR = `data:image/svg+xml,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256">'
  + '<defs>'
  + '<linearGradient id="v" x1="0" y1="0" x2="0" y2="1">'
  + '<stop offset="0" stop-color="#fff" stop-opacity="0.95"/>'
  + '<stop offset="0.45" stop-color="#fff" stop-opacity="0.3"/>'
  + '<stop offset="1" stop-color="#fff" stop-opacity="0.5"/>'
  + '</linearGradient>'
  + `${featherFilterDef('f', 16)}`
  + '</defs>'
  + `<path filter="url(#f)" fill-rule="evenodd" fill="url(#v)" d="${RIM_OUTER} ${RIM_INNER}"/>`
  + '</svg>',
)}`

/* 同一套折射/环形几何，明暗各注册一个滤镜，仅镜面提取矩阵不同：
   亮色阈值高（只取真正的亮部）；暗色增益略高、阈值略低 ——
   暗景高光更弱，需适度放大才可见，但增益过头会让整卡发光泛亮
   （输入是模糊后的 frost，放大不会引入噪点） */
function LiquidLensFilter({
  id,
  specMatrix,
  toneTable,
  bloom = false,
}: {
  id: string
  specMatrix: string
  /**
   * 链末压肩色调曲线（feComponentTransfer table，5 档采样）。
   * 暗色烟熏玻璃用：brightness 线性缩放压不住高亮壁纸（白×0.6 仍是亮灰），
   * 压肩曲线让高光被非线性吸收 —— 白色背景透过玻璃只剩 ~50%，
   * 中间调压得更深，暗部保持。亮色滤镜不传即无此级
   */
  toneTable?: string
  /**
   * 光晕 halation：从 frost 以高阈值（~0.75，只取真实光源级亮部）
   * 提取、大半径软化后 screen 叠加在压肩之后 —— 亮光源隔着玻璃
   * 在整个玻璃面上泛出柔光；置于压肩之后是因为光在玻璃内的散射
   * 不随烟熏吸收衰减。亮色场景整体高亮、bloom 只会糊成白雾，故仅暗色启用
   */
  bloom?: boolean
}) {
  return (
    <filter
      id={id}
      x="0"
      y="0"
      width="100%"
      height="100%"
      colorInterpolationFilters="sRGB"
    >
      <feImage href={LENS_MAP} preserveAspectRatio="none" result="map" />
      <feGaussianBlur in="SourceGraphic" stdDeviation="2" result="frost" />
      {/* 色散折射：RGB 按不同弯折率位移（蓝折射率最高，物理正序
          R50 < G56 < B62），弯折强处出现真实彩色镶边。
          性能：只做两次位移采样（50 与 62），绿通道取两者均值 ——
          对平滑位移场 ½·disp@50 + ½·disp@62 与 disp@56 一阶等价，
          误差为二阶小量（亚像素且被后续 3px 软化吞没）。
          两层各保 alpha=1、色道权重互补（G 各 0.5），arithmetic
          相加即无损合成 */}
      <feDisplacementMap
        in="SourceGraphic"
        in2="map"
        scale="50"
        xChannelSelector="R"
        yChannelSelector="G"
        result="dispA0"
      />
      <feColorMatrix
        in="dispA0"
        type="matrix"
        values="1 0 0 0 0  0 0.5 0 0 0  0 0 0 0 0  0 0 0 1 0"
        result="dispA"
      />
      <feDisplacementMap
        in="SourceGraphic"
        in2="map"
        scale="62"
        xChannelSelector="R"
        yChannelSelector="G"
        result="dispB0"
      />
      <feColorMatrix
        in="dispB0"
        type="matrix"
        values="0 0 0 0 0  0 0.5 0 0 0  0 0 1 0 0  0 0 0 1 0"
        result="dispB"
      />
      <feComposite
        in="dispA"
        in2="dispB"
        operator="arithmetic"
        k1="0"
        k2="1"
        k3="1"
        k4="0"
        result="bent0"
      />
      <feGaussianBlur in="bent0" stdDeviation="3" result="bent" />
      <feImage href={LENS_RIM} preserveAspectRatio="none" result="rim" />
      <feComposite in="bent" in2="rim" operator="in" result="rimbent" />
      <feMerge result="base">
        <feMergeNode in="frost" />
        <feMergeNode in="rimbent" />
      </feMerge>
      <feColorMatrix in="frost" type="matrix" values={specMatrix} result="spec0" />
      <feImage
        href={LENS_SPECULAR}
        preserveAspectRatio="none"
        result="specm"
      />
      <feComposite in="spec0" in2="specm" operator="in" result="spec" />
      <feBlend in="spec" in2="base" mode="screen" result="lit" />
      {toneTable && (
        <feComponentTransfer in="lit" result="toned">
          <feFuncR type="table" tableValues={toneTable} />
          <feFuncG type="table" tableValues={toneTable} />
          <feFuncB type="table" tableValues={toneTable} />
        </feComponentTransfer>
      )}
      {bloom && (
        <>
          <feColorMatrix
            in="frost"
            type="matrix"
            values="2 0 0 0 -1.5  0 2 0 0 -1.5  0 0 2 0 -1.5  0 0 0 1 0"
            result="bloom0"
          />
          <feGaussianBlur in="bloom0" stdDeviation="16" result="bloomSoft" />
          <feBlend
            in="bloomSoft"
            in2={toneTable ? 'toned' : 'lit'}
            mode="screen"
          />
        </>
      )}
    </filter>
  )
}

export function SurfaceThemeApplier() {
  useWidgetTheme()

  useEffect(() => {
    document.documentElement.classList.toggle(
      'surface-lens',
      'userAgentData' in navigator,
    )
  }, [])

  return (
    <svg
      width="0"
      height="0"
      style={{ position: 'absolute' }}
      aria-hidden="true"
    >
      <LiquidLensFilter
        id="liquid-lens"
        specMatrix="2 0 0 0 -0.55  0 2 0 0 -0.55  0 0 2 0 -0.55  0 0 0 1 0"
      />
      <LiquidLensFilter
        id="liquid-lens-dark"
        specMatrix="2.2 0 0 0 -0.4  0 2.2 0 0 -0.4  0 0 2.2 0 -0.4  0 0 0 1 0"
        toneTable="0 0.16 0.3 0.42 0.5"
        bloom
      />
    </svg>
  )
}

export default SurfaceThemeApplier
