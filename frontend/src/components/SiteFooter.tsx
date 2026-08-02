/**
 * 站点底部信息组件
 * 显示版本号、备案号、云基础设施 Logo、自定义图标+文本
 *
 * 注意：DOM class 避免使用 sponsor/ad 等易被广告拦截规则误杀的词。
 * 折叠逻辑：非首页或移动端 → compact（仅图标 + tooltip）
 */

import type { FooterCustomItem } from '../utils/footerCustomLogic'

import { SiCloudflare } from '@lib/icons'

import React, { memo, useCallback, useEffect, useState } from 'react'
import { getBuildInfo } from '../utils/buildInfo'
import {

  isFooterCustomHref,
  parseFooterCustom,
} from '../utils/footerCustomLogic'
import { getUIConfigDeduped } from '../utils/requestDedup'
import './SiteFooter.css'

interface SiteConfig {
  site_icp?: string
  site_gongan?: string
  cloud_sponsors?: string
  site_footer_custom?: string
}

// 又拍云 Logo（官方 logo 主体，移除文字）
const UpyunLogo: React.FC<{ className?: string }> = ({ className }) => (
  <svg viewBox="195 270 100 135" className={className}>
    <path
      fill="currentColor"
      d="M282.639,281.223L282.639,281.223L282.639,281.223
L282.639,281.223L282.639,281.223
                c-2.473-1.861-5.034-3.52-7.664-4.983c-1.904-1.059-4.295-0.564-5.604,1.177l-16.492,21.912l-1.176,1.563
                c-1.786,2.373-4.638,3.665-7.605,3.529c-1.082-0.049-2.164-0.039-3.242,0.029c-8.289,0.525-16.308,4.525-21.694,11.681
                c-4.33,5.753-6.229,12.576-5.879,19.245c0.063,1.201,0.757,2.298,1.851,2.796c2.27,1.032,4.017,3.137,4.454,5.83
                c0.618,3.809-1.722,7.551-5.418,8.659c-4.357,1.306-8.832-1.363-9.814-5.724c-0.532-2.362,0.079-4.711,1.463-6.48
                c0.768-0.981,1.154-2.203,1.044-3.444c-0.758-8.552,1.52-17.402,7.09-24.802c7.026-9.334,17.717-14.274,28.562-14.337
                c2.09-0.012,4.052-1.011,5.309-2.681l15.188-20.178c1.189-1.579,0.421-3.848-1.476-4.405c-25.43-7.46-53.905,0.934-70.96,23.146
                c-22.099,28.781-16.729,70.279,11.987,92.462c2.687,2.076,5.482,3.909,8.36,5.508c1.893,1.052,4.274,0.537,5.577-1.193
                l16.492-21.911l1.176-1.563c1.786-2.373,4.638-3.665,7.605-3.529c1.082,0.049,2.164,0.039,3.242-0.029
                c8.289-0.525,16.308-4.525,21.694-11.681c4.33-5.753,6.229-12.576,5.879-19.245c-0.063-1.201-0.757-2.298-1.851-2.796
                c-2.27-1.032-4.017-3.137-4.454-5.83c-0.618-3.809,1.722-7.551,5.418-8.658c4.357-1.306,8.832,1.363,9.814,5.724
                c0.532,2.362-0.079,4.711-1.463,6.48c-0.768,0.981-1.154-2.203-1.044-3.444c0.758,8.552-1.52,17.402-7.09,24.802
                c-7.026,9.334-17.717,14.274-28.562,14.337c-2.09,0.012-4.052-1.011-5.309-2.681l-15.187,20.177
                c-1.18,1.568-0.439,3.842,1.444,4.396c25.633,7.534,54.368-1.045,71.384-23.652C317.614,344.545,311.773,303.151,282.639,281.223z"
    />
  </svg>
)

// EdgeOne Logo
const EdgeOneLogo: React.FC<{ className?: string }> = ({ className }) => (
  <svg viewBox="0 0 32 32" fill="none" className={className}>
    <path
      d="M29.8101 18.138C29.9349 17.4442 30 16.7297 30 16C30 15.3831 29.9535 14.7772 29.8637 14.1854C29.829 13.9567 29.6296 13.792 29.3983 13.792H21.6802C21.4277 13.792 21.2439 13.5525 21.3093 13.3086L22.2229 9.89892C22.2904 9.6471 22.5185 9.472 22.7792 9.472H27.3634C27.668 9.472 27.8488 9.13574 27.6682 8.89047C25.4834 5.9244 21.9664 4 18 4C11.3726 4 6 9.37258 6 16C6 19.0173 7.11361 21.7745 8.95224 23.883C9.14804 24.1076 9.5076 24.0146 9.58436 23.7268L12.2394 13.7702C12.2882 13.5874 12.1504 13.408 11.9612 13.408H9.65504C9.40274 13.408 9.21899 13.1689 9.284 12.9251L10.0327 10.1174C10.0889 9.90673 10.28 9.76117 10.498 9.75666C13.0104 9.70465 15.493 9.04698 17.6975 7.84351C17.9253 7.71913 18.2007 7.92739 18.1338 8.1782L13.2499 26.4929C13.177 26.7664 13.313 27.0538 13.5761 27.1582C14.9451 27.7014 16.4377 28 18 28C21.7878 28 25.1656 26.2451 27.3649 23.5039C27.5597 23.2611 27.3809 22.912 27.0696 22.912H19.2365C18.984 22.912 18.8002 22.6725 18.8656 22.4286L19.7792 19.0189C19.8467 18.7671 20.0749 18.592 20.3356 18.592H29.2564C29.5268 18.592 29.7622 18.4042 29.8101 18.138Z"
      fill="#0055D2"
    />
  </svg>
)

// 云服务商 Logo 配置（config key 仍为 cloud_sponsors；class 用中性名避免广告插件误杀）
const CLOUD_PROVIDERS: Record<
  string,
  { name: string; icon: React.ReactNode; url: string; className: string }
> = {
  cloudflare: {
    name: 'Cloudflare',
    url: 'https://www.cloudflare.com',
    className: 'cdn-cloudflare',
    icon: <SiCloudflare />,
  },
  edgeone: {
    name: 'EdgeOne',
    url: 'https://cloud.tencent.com/product/eo',
    className: 'cdn-edgeone',
    icon: <EdgeOneLogo />,
  },
  upyun: {
    name: '又拍云',
    url: 'https://www.upyun.com',
    className: 'cdn-upyun',
    icon: <UpyunLogo />,
  },
}

// 优雅的 Tooltip 组件
const Tooltip: React.FC<{ content: string; children: React.ReactNode }> = ({
  content,
  children,
}) => (
  <div className="tooltip-wrapper">
    {children}
    <div className="tooltip-content">{content}</div>
  </div>
)

function CustomIcon({
  icon,
  alt,
  className,
}: {
  icon: string
  alt: string
  className?: string
}) {
  if (!icon) {
    return <span className={`text-icon ${className || ''}`}>★</span>
  }
  return (
    <img
      src={icon}
      alt={alt}
      className={`footer-custom-icon ${className || ''}`}
      loading="lazy"
      decoding="async"
    />
  )
}

interface SiteFooterProps {
  isHomePage?: boolean
}

export const SiteFooter: React.FC<SiteFooterProps> = memo(
  ({ isHomePage = false }) => {
    const [config, setConfig] = useState<SiteConfig | null>(null)
    const [isMobile, setIsMobile] = useState(false)
    const buildInfo = getBuildInfo()

    // 检测移动端（带防抖，避免拖拽窗口时频繁 setState）
    useEffect(() => {
      const checkMobile = () => setIsMobile(window.innerWidth < 768)
      checkMobile()
      let timerId: ReturnType<typeof setTimeout>
      const handleResize = () => {
        clearTimeout(timerId)
        timerId = setTimeout(checkMobile, 150)
      }
      window.addEventListener('resize', handleResize, { passive: true })
      return () => {
        window.removeEventListener('resize', handleResize)
        clearTimeout(timerId)
      }
    }, [])

    const loadConfig = useCallback(async () => {
      try {
        const data = await getUIConfigDeduped()
        setConfig(data as SiteConfig)
      } catch {
        /* silent */
      }
    }, [])

    useEffect(() => {
      void loadConfig()
      const onFooterChanged = () => {
        void loadConfig()
      }
      window.addEventListener('footerConfigChanged', onFooterChanged)
      return () => {
        window.removeEventListener('footerConfigChanged', onFooterChanged)
      }
    }, [loadConfig])

    // 解析云基础设施展示（config: cloud_sponsors）
    const providers = config?.cloud_sponsors
      ? config.cloud_sponsors
          .split(',')
          .map((s) => s.trim().toLowerCase())
          .filter((s) => CLOUD_PROVIDERS[s])
      : []

    const customItems: FooterCustomItem[] = parseFooterCustom(
      config?.site_footer_custom,
    )

    // 如果没有任何内容要显示，不渲染（完整模式仍有版本号）
    const hasContent =
      config?.site_icp ||
      config?.site_gongan ||
      providers.length > 0 ||
      customItems.length > 0

    // 移动端强制使用简化模式
    const useCompactMode = !isHomePage || isMobile

    const renderCustomCompact = () =>
      customItems.map((item, i) => {
        const body = (
          <span className="footer-icon-link footer-icon-custom">
            <CustomIcon icon={item.icon} alt={item.text} />
          </span>
        )
        const wrapped = (
          <Tooltip key={`custom-${i}`} content={item.text}>
            {isFooterCustomHref(item.url) ? (
              <a
                href={item.url}
                target="_blank"
                rel="noopener noreferrer"
                className="footer-custom-hit"
              >
                {body}
              </a>
            ) : (
              body
            )}
          </Tooltip>
        )
        return wrapped
      })

    const renderCustomFull = () =>
      customItems.map((item, i) => {
        const inner = (
          <>
            <CustomIcon icon={item.icon} alt="" />
            <span className="footer-custom-text">{item.text}</span>
          </>
        )
        if (isFooterCustomHref(item.url)) {
          return (
            <a
              key={`custom-${i}`}
              href={item.url}
              target="_blank"
              rel="noopener noreferrer"
              className="footer-custom-link"
            >
              {inner}
            </a>
          )
        }
        return (
          <span key={`custom-${i}`} className="footer-custom-link">
            {inner}
          </span>
        )
      })

    // 简化模式（非首页或移动端）：只显示图标
    if (useCompactMode) {
      const hasAnyIcon =
        config?.site_icp ||
        config?.site_gongan ||
        providers.length > 0 ||
        customItems.length > 0
      if (!hasAnyIcon) return null

      return (
        <footer className="site-footer site-footer-compact">
          <div className="site-footer-content">
            {/* 备案信息图标 */}
            {config?.site_icp && (
              <Tooltip content={config.site_icp}>
                <a
                  href="https://beian.miit.gov.cn/"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="footer-icon-link footer-icon-icp"
                >
                  <span className="text-icon">备</span>
                </a>
              </Tooltip>
            )}

            {config?.site_gongan && (
              <Tooltip content={config.site_gongan}>
                <a
                  href="http://www.beian.gov.cn/"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="footer-icon-link footer-icon-gongan"
                >
                  <span className="text-icon">公</span>
                </a>
              </Tooltip>
            )}

            {/* 云基础设施图标 */}
            {providers.map((key) => {
              const provider = CLOUD_PROVIDERS[key]
              return (
                <Tooltip key={key} content={provider.name}>
                  <a
                    href={provider.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className={`infra-link ${provider.className}`}
                  >
                    {provider.icon}
                  </a>
                </Tooltip>
              )
            })}

            {renderCustomCompact()}
          </div>
        </footer>
      )
    }

    // 完整模式（首页）
    return (
      <footer className="site-footer">
        <div className="site-footer-content">
          {/* 版本号 */}
          <div className="footer-version">
            <span className="version-label">Myriad</span>
            {buildInfo.commitUrl ? (
              <a
                className="version-number version-link"
                href={buildInfo.commitUrl}
                target="_blank"
                rel="noreferrer noopener"
                title={buildInfo.commitSha ?? undefined}
              >
                {buildInfo.version} · {buildInfo.commitSha?.slice(0, 7)}
              </a>
            ) : (
              <span className="version-number">{buildInfo.version}</span>
            )}
          </div>

          {/* 分隔符 */}
          {hasContent && <span className="footer-divider">·</span>}

          {/* 备案信息 */}
          {config?.site_icp && (
            <a
              href="https://beian.miit.gov.cn/"
              target="_blank"
              rel="noopener noreferrer"
              className="footer-icp"
            >
              {config.site_icp}
            </a>
          )}

          {config?.site_gongan && (
            <a
              href="http://www.beian.gov.cn/"
              target="_blank"
              rel="noopener noreferrer"
              className="footer-gongan"
            >
              <span className="gongan-text-icon">公</span>
              {config.site_gongan}
            </a>
          )}

          {/* 云基础设施 */}
          {providers.length > 0 && (
            <>
              <span className="footer-divider">·</span>
              <div className="footer-infra">
                <span className="infra-label">Powered by</span>
                {providers.map((key) => {
                  const provider = CLOUD_PROVIDERS[key]
                  return (
                    <a
                      key={key}
                      href={provider.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className={`infra-link ${provider.className}`}
                      title={provider.name}
                    >
                      {provider.icon}
                    </a>
                  )
                })}
              </div>
            </>
          )}

          {/* 自定义区块 */}
          {customItems.length > 0 && (
            <>
              <span className="footer-divider">·</span>
              <div className="footer-custom">{renderCustomFull()}</div>
            </>
          )}
        </div>
      </footer>
    )
  },
)

SiteFooter.displayName = 'SiteFooter'

export default SiteFooter
