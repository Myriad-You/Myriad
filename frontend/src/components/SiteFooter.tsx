/**
 * 站点底部信息组件
 * 显示版本号、备案号、云赞助商 Logo
 */

import { SiCloudflare } from '@lib/icons'
import React, { useEffect, useState } from 'react'
import { API_URL } from '../config'
import { fetchJson } from '../utils/apiHelper'
import './SiteFooter.css'

// Myriad 版本号 - 从 package.json 读取
const MYRIAD_VERSION = __APP_VERSION__ || '0.1.0'

interface SiteConfig {
  site_icp?: string
  site_gongan?: string
  cloud_sponsors?: string
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
                c0.532,2.362-0.079,4.711-1.463,6.48c-0.768,0.981-1.154,2.203-1.044,3.444c0.758,8.552-1.52,17.402-7.09,24.802
                c-7.026,9.334-17.717,14.274-28.562,14.337c-2.09,0.012-4.052,1.011-5.309,2.681l-15.187,20.177
                c-1.18,1.568-0.439,3.842,1.444,4.396c25.633,7.534,54.368-1.045,71.384-23.652C317.614,344.545,311.773,303.151,282.639,281.223z"
    />
  </svg>
)

// EdgeOne Logo（蓝色盾牌 + 闪电）
const EdgeOneLogo: React.FC<{ className?: string }> = ({ className }) => (
  <svg viewBox="0 0 24 24" className={className}>
    <defs>
      <linearGradient id="edgeone-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" stopColor="#00D4FF" />
        <stop offset="100%" stopColor="#0066FF" />
      </linearGradient>
    </defs>
    {/* 盾牌外形 */}
    <path
      fill="url(#edgeone-gradient)"
      d="M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4z"
    />
    {/* 闪电 */}
    <path
      fill="#FFFFFF"
      d="M13 6L8 13h3v5l5-7h-3V6z"
    />
  </svg>
)

// 云服务商 Logo 配置
const CLOUD_SPONSORS: Record<string, { name: string, icon: React.ReactNode, url: string, className: string }> = {
  cloudflare: {
    name: 'Cloudflare',
    url: 'https://www.cloudflare.com',
    className: 'sponsor-cloudflare',
    icon: <SiCloudflare />,
  },
  edgeone: {
    name: 'EdgeOne',
    url: 'https://cloud.tencent.com/product/eo',
    className: 'sponsor-edgeone',
    icon: <EdgeOneLogo />,
  },
  upyun: {
    name: '又拍云',
    url: 'https://www.upyun.com',
    className: 'sponsor-upyun',
    icon: <UpyunLogo />,
  },
}

// 优雅的 Tooltip 组件
const Tooltip: React.FC<{ content: string, children: React.ReactNode }> = ({ content, children }) => (
  <div className="tooltip-wrapper">
    {children}
    <div className="tooltip-content">
      {content}
    </div>
  </div>
)

interface SiteFooterProps {
  isHomePage?: boolean
}

export const SiteFooter: React.FC<SiteFooterProps> = ({ isHomePage = false }) => {
  const [config, setConfig] = useState<SiteConfig | null>(null)
  const [isMobile, setIsMobile] = useState(false)

  // 检测移动端
  useEffect(() => {
    const checkMobile = () => setIsMobile(window.innerWidth < 768)
    checkMobile()
    window.addEventListener('resize', checkMobile)
    return () => window.removeEventListener('resize', checkMobile)
  }, [])

  useEffect(() => {
    const loadConfig = async () => {
      try {
        const data = await fetchJson(`${API_URL}/api/config/ui`, {
          credentials: 'include',
        })
        console.debug('[SiteFooter] Config loaded:', data)
        setConfig(data)
      }
      catch (e) {
        console.debug('[SiteFooter] Failed to load config:', e)
      }
    }
    loadConfig()
  }, [])

  // 解析云赞助商
  const sponsors = config?.cloud_sponsors
    ? config.cloud_sponsors.split(',').map(s => s.trim().toLowerCase()).filter(s => CLOUD_SPONSORS[s])
    : []

  // 如果没有任何内容要显示，不渲染
  const hasContent = config?.site_icp || config?.site_gongan || sponsors.length > 0

  // 移动端强制使用简化模式
  const useCompactMode = !isHomePage || isMobile

  // 简化模式（非首页或移动端）：只显示图标
  if (useCompactMode) {
    const hasAnyIcon = config?.site_icp || config?.site_gongan || sponsors.length > 0
    if (!hasAnyIcon)
      return null

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

          {/* 云赞助商图标 */}
          {sponsors.map((key) => {
            const sponsor = CLOUD_SPONSORS[key]
            return (
              <Tooltip key={key} content={sponsor.name}>
                <a
                  href={sponsor.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className={`sponsor-link ${sponsor.className}`}
                >
                  {sponsor.icon}
                </a>
              </Tooltip>
            )
          })}
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
          <span className="version-number">
            v
            {MYRIAD_VERSION}
          </span>
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

        {/* 云赞助商 */}
        {sponsors.length > 0 && (
          <>
            <span className="footer-divider">·</span>
            <div className="footer-sponsors">
              <span className="sponsors-label">Powered by</span>
              {sponsors.map((key) => {
                const sponsor = CLOUD_SPONSORS[key]
                return (
                  <a
                    key={key}
                    href={sponsor.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className={`sponsor-link ${sponsor.className}`}
                    title={sponsor.name}
                  >
                    {sponsor.icon}
                  </a>
                )
              })}
            </div>
          </>
        )}
      </div>
    </footer>
  )
}

export default SiteFooter
