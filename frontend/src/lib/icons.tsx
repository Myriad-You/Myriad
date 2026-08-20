/**
 * 统一图标导出
 * 从 react-icons 和 lucide-react 精确导入需要的图标，保持包体积最小化
 *
 * ⚠️ 重要：不要使用 import * as 语法，否则会导入整个图标库
 */

import type { LucideIcon } from 'lucide-react'
import type { IconType } from 'react-icons'
// Lucide Icons (Lu*) - 用于 Brew 阅读器等现代 UI
import {
  Activity as LuActivity,
  AlertCircle as LuAlertCircle,
  AlertTriangle as LuAlertTriangle,
  AlignJustify as LuAlignJustify,
  ArrowRight as LuArrowRight,
  ArrowUpDown as LuArrowUpDown,
  BarChart3 as LuBarChart3,
  BookOpen as LuBookOpen,
  Calendar as LuCalendar,
  Check as LuCheck,
  CircleDashed as LuCircleDashed,
  CheckCircle as LuCheckCircle,
  CheckSquare as LuCheckSquare,
  ChevronDown as LuChevronDown,
  ChevronLeft as LuChevronLeft,
  ChevronRight as LuChevronRight,
  ChevronUp as LuChevronUp,
  ClipboardList as LuClipboardList,
  Clock as LuClock,
  Cloud as LuCloud,
  CloudFog as LuCloudFog,
  CloudRain as LuCloudRain,
  CloudSnow as LuCloudSnow,
  CloudSun as LuCloudSun,
  Copy as LuCopy,
  Cpu as LuCpu,
  Crown as LuCrown,
  Database as LuDatabase,
  Download as LuDownload,
  Droplets as LuDroplets,
  Edit3 as LuEdit3,
  ExternalLink as LuExternalLink,
  Eye as LuEye,
  EyeOff as LuEyeOff,
  FileText as LuFileText,
  Folder as LuFolder,
  FolderOpen as LuFolderOpen,
  Gauge as LuGauge,
  GitFork as LuGitFork,
  Globe as LuGlobe,
  GripVertical as LuGripVertical,
  Headphones as LuHeadphones,
  Heart as LuHeart,
  Image as LuImage,
  Inbox as LuInbox,
  Info as LuInfo,
  Key as LuKey,
  Keyboard as LuKeyboard,
  Languages as LuLanguages,
  Leaf as LuLeaf,
  Link as LuLink,
  List as LuList,
  ListMusic as LuListMusic,
  Loader2 as LuLoader2,
  Mars as LuMars,
  Lock as LuLock,
  MemoryStick as LuMemoryStick,
  MessageCircle as LuMessageCircle,
  MessageSquare as LuMessageSquare,
  Mic as LuMic,
  Minus as LuMinus,
  MinusSquare as LuMinusSquare,
  Monitor as LuMonitor,
  Moon as LuMoon,
  MoreHorizontal as LuMoreHorizontal,
  Music as LuMusic,
  Music2 as LuMusic2,
  Newspaper as LuNewspaper,
  Package as LuPackage,
  Palette as LuPalette,
  Pause as LuPause,
  Pin as LuPin,
  PinOff as LuPinOff,
  Play as LuPlay,
  Plus as LuPlus,
  RefreshCw as LuRefreshCw,
  Reply as LuReply,
  RotateCw as LuRotateCw,
  Rss as LuRss,
  Scale as LuScale,
  Search as LuSearch,
  SearchX as LuSearchX,
  Send as LuSend,
  Server as LuServer,
  Settings as LuSettings,
  ShieldAlert as LuShieldAlert,
  ShieldCheck as LuShieldCheck,
  ShoppingCart as LuShoppingCart,
  Shuffle as LuShuffle,
  SkipBack as LuSkipBack,
  SkipForward as LuSkipForward,
  SlidersHorizontal as LuSlidersHorizontal,
  SortAsc as LuSortAsc,
  Sparkles as LuSparkles,
  Square as LuSquare,
  Star as LuStar,
  Store as LuStore,
  Sun as LuSun,
  Sunrise as LuSunrise,
  Sunset as LuSunset,
  Tag as LuTag,
  Transgender as LuTransgender,
  Trash2 as LuTrash2,
  Type as LuType,
  Upload as LuUpload,
  User as LuUser,
  Users as LuUsers,
  Venus as LuVenus,
  Video as LuVideo,
  Volume2 as LuVolume2,
  Wind as LuWind,
  Wrench as LuWrench,
  X as LuX,
  Zap as LuZap,
  ZoomIn as LuZoomIn,
  ZoomOut as LuZoomOut,
} from 'lucide-react'

import { BsNintendoSwitch } from 'react-icons/bs'
// Font Awesome Icons (Fa*) - 通用图标
import {
  FaAlipay,
  FaAmazon,
  FaAnchor,
  FaArrowDown,
  FaArrowLeft,
  FaArrowRight,
  FaArrowUp,
  FaBars,
  FaBell,
  FaBellSlash,
  FaBlog,
  FaBolt,
  FaBook,
  FaBrain,
  FaBroadcastTower,
  FaBug,
  FaCalendar,
  FaCalendarAlt,
  FaCamera,
  FaChartBar,
  FaChartLine,
  FaChartPie,
  FaCheck,
  FaCheckCircle,
  FaChevronDown,
  FaChevronLeft,
  FaChevronRight,
  FaChevronUp,
  FaClipboard,
  FaClock,
  FaCloud,
  FaCode,
  FaCodepen,
  FaCoffee,
  FaCog,
  FaComments,
  FaCompass,
  FaCompress,
  FaCopy,
  FaCrown,
  FaDatabase,
  FaDownload,
  FaEdit,
  FaEllipsisH,
  FaEllipsisV,
  FaEnvelope,
  FaExchangeAlt,
  FaExclamationCircle,
  FaExclamationTriangle,
  FaExpand,
  FaExternalLinkAlt,
  FaEye,
  FaEyeSlash,
  FaFeather,
  FaFileAlt,
  FaFilter,
  FaFire,
  FaFolder,
  FaFolderOpen,
  FaFreeCodeCamp,
  FaGamepad,
  FaGem,
  FaGithub,
  FaGlobe,
  FaGraduationCap,
  FaGripHorizontal,
  FaGripVertical,
  FaHdd,
  FaHeadphones,
  FaHeart,
  FaHistory,
  FaHome,
  FaImage,
  FaImages,
  FaInfoCircle,
  FaKey,
  FaLeaf,
  FaLightbulb,
  FaLink,
  FaLinkedin,
  FaList,
  FaLock,
  FaMagic,
  FaMicrophone,
  FaMoon,
  FaMountain,
  FaMusic,
  FaNewspaper,
  FaPaintBrush,
  FaPalette,
  FaPaperPlane,
  FaPause,
  FaPlay,
  FaPlayCircle,
  FaPlus,
  FaQuestionCircle,
  FaRedo,
  FaRobot,
  FaRocket,
  FaSave,
  FaSearch,
  FaServer,
  FaShoppingCart,
  FaSignInAlt,
  FaSignOutAlt,
  FaSlidersH,
  FaSnowflake,
  FaSort,
  FaSortDown,
  FaSortUp,
  FaSpinner,
  FaStar,
  FaSteam,
  FaSun,
  FaSync,
  FaSyncAlt,
  FaTerminal,
  FaTh,
  FaThLarge,
  FaTimes,
  FaTimesCircle,
  FaTools,
  FaTrash,
  FaTwitter,
  FaUmbrella,
  FaUndo,
  FaUnlink,
  FaUnlock,
  FaUpload,
  FaUser,
  FaUsers,
  FaVideo,
  FaVolumeUp,
  FaWater,
  FaWrench,
  FaXbox,
} from 'react-icons/fa'
// Font Awesome 6 Icons (Fa6*) - 用于 FaGrip 等
import { FaGrip } from 'react-icons/fa6'
// Simple Icons (Si*) - 品牌图标
import {
  SiAnilist as SiAnilistRaw,
  SiApple as SiAppleRaw,
  SiArtstation as SiArtstationRaw,
  SiBaidu as SiBaiduRaw,
  SiBandcamp as SiBandcampRaw,
  SiBehance as SiBehanceRaw,
  SiBilibili as SiBilibiliRaw,
  SiBitbucket as SiBitbucketRaw,
  SiBlogger as SiBloggerRaw,
  SiBluesky as SiBlueskyRaw,
  SiBuymeacoffee as SiBuymeacoffeeRaw,
  SiCloudflare as SiCloudflareRaw,
  SiDevdotto as SiDevdottoRaw,
  SiDeviantart as SiDeviantartRaw,
  SiDiscord as SiDiscordRaw,
  SiDouban as SiDoubanRaw,
  SiDribbble as SiDribbbleRaw,
  SiEpicgames as SiEpicgamesRaw,
  SiFacebook as SiFacebookRaw,
  SiFigma as SiFigmaRaw,
  SiFlickr as SiFlickrRaw,
  SiGithub as SiGithubRaw,
  SiGitlab as SiGitlabRaw,
  SiGoodreads as SiGoodreadsRaw,
  SiGooglegemini as SiGooglegeminiRaw,
  SiGoogle as SiGoogleRaw,
  SiHashnode as SiHashnodeRaw,
  SiHuggingface as SiHuggingfaceRaw,
  SiInstagram as SiInstagramRaw,
  SiItchdotio as SiItchdotioRaw,
  SiKaggle as SiKaggleRaw,
  SiKakaotalk as SiKakaotalkRaw,
  SiKofi as SiKofiRaw,
  SiLastdotfm as SiLastdotfmRaw,
  SiLetterboxd as SiLetterboxdRaw,
  SiLine as SiLineRaw,
  SiMaildotru as SiMaildotruRaw,
  SiMastodon as SiMastodonRaw,
  SiMedium as SiMediumRaw,
  SiMisskey as SiMisskeyRaw,
  SiMyanimelist as SiMyanimelistRaw,
  SiNaver as SiNaverRaw,
  SiNeteasecloudmusic as SiNeteasecloudmusicRaw,
  SiNiconico as SiNiconicoRaw,
  SiNotion as SiNotionRaw,
  SiOdnoklassniki as SiOdnoklassnikiRaw,
  SiOpenrouter as SiOpenrouterRaw,
  SiOrigin as SiOriginRaw,
  SiPatreon as SiPatreonRaw,
  SiPinterest as SiPinterestRaw,
  SiPixiv as SiPixivRaw,
  SiPlaystation as SiPlaystationRaw,
  SiProducthunt as SiProducthuntRaw,
  SiQq as SiQqRaw,
  SiReddit as SiRedditRaw,
  SiSinaweibo as SiSinaweiboRaw,
  SiSnapchat as SiSnapchatRaw,
  SiSoundcloud as SiSoundcloudRaw,
  SiSpotify as SiSpotifyRaw,
  SiStackoverflow as SiStackoverflowRaw,
  SiSteam as SiSteamRaw,
  SiSubstack as SiSubstackRaw,
  SiTelegram as SiTelegramRaw,
  SiThreads as SiThreadsRaw,
  SiTiktok as SiTiktokRaw,
  SiTrakt as SiTraktRaw,
  SiTumblr as SiTumblrRaw,
  SiTwitch as SiTwitchRaw,
  SiUnsplash as SiUnsplashRaw,
  SiVk as SiVkRaw,
  SiWechat as SiWechatRaw,
  SiWhatsapp as SiWhatsappRaw,
  SiWordpress as SiWordpressRaw,
  SiXiaohongshu as SiXiaohongshuRaw,
  SiX as SiXRaw,
  SiYcombinator as SiYcombinatorRaw,
  SiYoutube as SiYoutubeRaw,
  SiZhihu as SiZhihuRaw,
} from 'react-icons/si'

/**
 * Decorative-by-default a11y for react-icons SVGs.
 *
 * Simple Icons bake role="img" into SVG attrs. Without title/aria-label that
 * fails Lighthouse "svg-img-alt". Most call sites use icons next to visible
 * labels, so default to aria-hidden and strip role=img. When the caller
 * provides an accessible name (title / aria-label / aria-labelledby), keep
 * role="img". Explicit role / aria-hidden from the caller always win.
 */
function withIconA11y(Icon: IconType): IconType {
  const AccessibleIcon: IconType = (props) => {
    const hasAccessibleName =
      (props.title != null && props.title !== '') ||
      (props['aria-label'] != null && props['aria-label'] !== '') ||
      (props['aria-labelledby'] != null && props['aria-labelledby'] !== '')

    const explicitAriaHidden = Object.hasOwn(props, 'aria-hidden')
    const explicitRole = Object.hasOwn(props, 'role')

    if (hasAccessibleName) {
      return (
        <Icon
          {...props}
          role={explicitRole ? props.role : 'img'}
          aria-hidden={explicitAriaHidden ? props['aria-hidden'] : undefined}
        />
      )
    }

    return (
      <Icon
        {...props}
        role={explicitRole ? props.role : undefined}
        aria-hidden={explicitAriaHidden ? props['aria-hidden'] : true}
      />
    )
  }

  return AccessibleIcon
}

// Wrap all Simple Icons exports (decorative default)
const SiAnilist = withIconA11y(SiAnilistRaw)
const SiApple = withIconA11y(SiAppleRaw)
const SiArtstation = withIconA11y(SiArtstationRaw)
const SiBaidu = withIconA11y(SiBaiduRaw)
const SiBandcamp = withIconA11y(SiBandcampRaw)
const SiBehance = withIconA11y(SiBehanceRaw)
const SiBilibili = withIconA11y(SiBilibiliRaw)
const SiBitbucket = withIconA11y(SiBitbucketRaw)
const SiBlogger = withIconA11y(SiBloggerRaw)
const SiBluesky = withIconA11y(SiBlueskyRaw)
const SiBuymeacoffee = withIconA11y(SiBuymeacoffeeRaw)
const SiCloudflare = withIconA11y(SiCloudflareRaw)
const SiDevdotto = withIconA11y(SiDevdottoRaw)
const SiDeviantart = withIconA11y(SiDeviantartRaw)
const SiDiscord = withIconA11y(SiDiscordRaw)
const SiDouban = withIconA11y(SiDoubanRaw)
const SiDribbble = withIconA11y(SiDribbbleRaw)
const SiEpicgames = withIconA11y(SiEpicgamesRaw)
const SiFacebook = withIconA11y(SiFacebookRaw)
const SiFigma = withIconA11y(SiFigmaRaw)
const SiFlickr = withIconA11y(SiFlickrRaw)
const SiGithub = withIconA11y(SiGithubRaw)
const SiGitlab = withIconA11y(SiGitlabRaw)
const SiGoodreads = withIconA11y(SiGoodreadsRaw)
const SiGoogle = withIconA11y(SiGoogleRaw)
const SiGooglegemini = withIconA11y(SiGooglegeminiRaw)
const SiHashnode = withIconA11y(SiHashnodeRaw)
const SiHuggingface = withIconA11y(SiHuggingfaceRaw)
const SiInstagram = withIconA11y(SiInstagramRaw)
const SiItchdotio = withIconA11y(SiItchdotioRaw)
const SiKaggle = withIconA11y(SiKaggleRaw)
const SiKakaotalk = withIconA11y(SiKakaotalkRaw)
const SiKofi = withIconA11y(SiKofiRaw)
const SiLastdotfm = withIconA11y(SiLastdotfmRaw)
const SiLetterboxd = withIconA11y(SiLetterboxdRaw)
const SiLine = withIconA11y(SiLineRaw)
const SiMaildotru = withIconA11y(SiMaildotruRaw)
const SiMastodon = withIconA11y(SiMastodonRaw)
const SiMedium = withIconA11y(SiMediumRaw)
const SiMisskey = withIconA11y(SiMisskeyRaw)
const SiMyanimelist = withIconA11y(SiMyanimelistRaw)
const SiNaver = withIconA11y(SiNaverRaw)
const SiNeteasecloudmusic = withIconA11y(SiNeteasecloudmusicRaw)
const SiNiconico = withIconA11y(SiNiconicoRaw)
const SiNotion = withIconA11y(SiNotionRaw)
const SiOdnoklassniki = withIconA11y(SiOdnoklassnikiRaw)
const SiOpenrouter = withIconA11y(SiOpenrouterRaw)
const SiOrigin = withIconA11y(SiOriginRaw)
const SiPatreon = withIconA11y(SiPatreonRaw)
const SiPinterest = withIconA11y(SiPinterestRaw)
const SiPixiv = withIconA11y(SiPixivRaw)
const SiPlaystation = withIconA11y(SiPlaystationRaw)
const SiProducthunt = withIconA11y(SiProducthuntRaw)
const SiQq = withIconA11y(SiQqRaw)
const SiReddit = withIconA11y(SiRedditRaw)
const SiSinaweibo = withIconA11y(SiSinaweiboRaw)
const SiSnapchat = withIconA11y(SiSnapchatRaw)
const SiSoundcloud = withIconA11y(SiSoundcloudRaw)
const SiSpotify = withIconA11y(SiSpotifyRaw)
const SiStackoverflow = withIconA11y(SiStackoverflowRaw)
const SiSteam = withIconA11y(SiSteamRaw)
const SiSubstack = withIconA11y(SiSubstackRaw)
const SiTelegram = withIconA11y(SiTelegramRaw)
const SiThreads = withIconA11y(SiThreadsRaw)
const SiTiktok = withIconA11y(SiTiktokRaw)
const SiTrakt = withIconA11y(SiTraktRaw)
const SiTumblr = withIconA11y(SiTumblrRaw)
const SiTwitch = withIconA11y(SiTwitchRaw)
const SiUnsplash = withIconA11y(SiUnsplashRaw)
const SiVk = withIconA11y(SiVkRaw)
const SiWechat = withIconA11y(SiWechatRaw)
const SiWhatsapp = withIconA11y(SiWhatsappRaw)
const SiWordpress = withIconA11y(SiWordpressRaw)
const SiX = withIconA11y(SiXRaw)
const SiXiaohongshu = withIconA11y(SiXiaohongshuRaw)
const SiYcombinator = withIconA11y(SiYcombinatorRaw)
const SiYoutube = withIconA11y(SiYoutubeRaw)
const SiZhihu = withIconA11y(SiZhihuRaw)

const SiCodepen = withIconA11y(FaCodepen)

/**
 * OpenAI 品牌图标
 * Simple Icons 中已移除 SiOpenai（商标原因），使用官方 blossom 标志的自定义 SVG
 */
const OpenAiIconRaw: IconType = ({ size, style, title, ...props }) => {
  const iconSize = size ?? '1em'

  return (
    <svg
      viewBox="0 0 24 24"
      width={iconSize}
      height={iconSize}
      xmlns="http://www.w3.org/2000/svg"
      fill="currentColor"
      style={{ verticalAlign: 'middle', ...style }}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      <path d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z" />
    </svg>
  )
}

const OpenAiIcon = withIconA11y(OpenAiIconRaw)
const SiOpenai = OpenAiIcon

const BangumiIconRaw: IconType = ({ size, style, title, ...props }) => {
  const iconSize = size ?? '1em'

  return (
    <svg
      viewBox="0 0 24 24"
      width={iconSize}
      height={iconSize}
      xmlns="http://www.w3.org/2000/svg"
      style={{ verticalAlign: 'middle', ...style }}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      <g
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d="M8.55 6.75 5.95 2.25" strokeWidth="2.15" />
        <path d="M15.45 6.75 18.05 2.25" strokeWidth="2.15" />
        <path
          d="M4.25 6.85h15.5A2.95 2.95 0 0 1 22.7 9.8v7.25A2.95 2.95 0 0 1 19.75 20H11.35L6.8 23.05 7.85 20h-3.6a2.95 2.95 0 0 1-2.95-2.95V9.8a2.95 2.95 0 0 1 2.95-2.95Z"
          strokeWidth="2.05"
        />
        <path d="m5.6 11.35 3.35 1.35-3.35 1.35" strokeWidth="1.45" />
        <path d="m18.4 11.35-3.35 1.35 3.35 1.35" strokeWidth="1.45" />
        <path d="M9.75 13.2h4.5L12 16.95Z" strokeWidth="1.35" />
      </g>
    </svg>
  )
}

const BangumiIcon = withIconA11y(BangumiIconRaw)
const SiBangumi = BangumiIcon

/**
 * QQ 音乐品牌图标
 * Simple Icons 仅有 SiQq（QQ 通讯），无 QQ 音乐；使用官方圆标 + 音符剪影的单色 currentColor 版
 */
const QqMusicIconRaw: IconType = ({ size, style, title, ...props }) => {
  const iconSize = size ?? '1em'

  return (
    <svg
      viewBox="0 0 24 24"
      width={iconSize}
      height={iconSize}
      xmlns="http://www.w3.org/2000/svg"
      fill="currentColor"
      style={{ verticalAlign: 'middle', ...style }}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      <path
        fillRule="evenodd"
        d="M12 0c6.627 0 12 5.373 12 12s-5.373 12-12 12S0 18.627 0 12 5.373 0 12 0zM13.828 13.021C13.527 12.597 13.241 12.199 12.955 11.806 11.857 10.284 10.754 8.763 9.651 7.246 8.951 6.281 8.257 5.316 7.547 4.361 7.333 4.075 7.307 3.774 7.404 3.447 7.603 2.768 8.078 2.298 8.64 1.91 9.666 1.2 10.826.837 12.041.623 13.098.439 14.145.214 15.069-.368 15.319-.526 15.529-.74 15.758-.929 15.82-.98 15.876-1.037 15.993-1.139 16.07-.766 16.146-.454 16.197-.143 16.351.776 16.294 1.67 15.881 2.517 15.319 3.661 14.4 4.376 13.195 4.744 12.46 4.979 11.699 5.035 10.928 5.03 10.871 5.03 10.815 5.045 10.718 5.055 10.907 5.387 11.076 5.699 11.26 6 12 7.21 12.74 8.42 13.476 9.636l2.42 4.013c.21.348.429.695.633 1.047.475.817.822 1.67.756 2.65-.066.96-.414 1.798-1.011 2.533-.975 1.2-2.242 1.895-3.753 2.165-1.425.25-2.783.092-4.044-.628-1.42-.807-2.329-2.272-2.114-4.019.137-1.148.709-2.098 1.552-2.88.899-.827 1.966-1.332 3.156-1.562.878-.174 1.756-.164 2.624.071.026.006.056-.005.133-.005z"
      />
    </svg>
  )
}

const QqMusicIcon = withIconA11y(QqMusicIconRaw)
const SiQqmusic = QqMusicIcon

const MyriadStoreIconRaw: IconType = ({ size, style, title, ...props }) => {
  const iconSize = size ?? '1em'

  return (
    <svg
      viewBox="0 0 24 24"
      width={iconSize}
      height={iconSize}
      xmlns="http://www.w3.org/2000/svg"
      style={{ verticalAlign: 'middle', ...style }}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      <g
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        transform="translate(12 12) scale(1.16) translate(-12 -12)"
      >
        <path
          d="M6.55 5.35h10.9a1.85 1.85 0 0 1 1.78 1.35l.68 2.55H4.09l.68-2.55a1.85 1.85 0 0 1 1.78-1.35Z"
          strokeWidth="1.75"
        />
        <path
          d="M4.6 9.25v1.15a2.35 2.35 0 0 0 4.7 0V9.25"
          strokeWidth="1.75"
        />
        <path d="M9.3 9.25v1.15a2.7 2.7 0 0 0 5.4 0V9.25" strokeWidth="1.75" />
        <path
          d="M14.7 9.25v1.15a2.35 2.35 0 0 0 4.7 0V9.25"
          strokeWidth="1.75"
        />
        <path
          d="M5.75 13.25v4.45a1.95 1.95 0 0 0 1.95 1.95h8.6a1.95 1.95 0 0 0 1.95-1.95v-4.45"
          strokeWidth="1.75"
        />
      </g>
    </svg>
  )
}

const MyriadStoreIcon = withIconA11y(MyriadStoreIconRaw)

// 导出 Simple Icons
export {
  BangumiIcon,
  MyriadStoreIcon,
  QqMusicIcon,
  SiAnilist,
  SiApple,
  SiArtstation,
  SiBaidu,
  SiBandcamp,
  SiBangumi,
  SiBehance,
  SiBilibili,
  SiBitbucket,
  SiBlogger,
  SiBluesky,
  SiBuymeacoffee,
  SiCloudflare,
  SiCodepen,
  SiDevdotto,
  SiDeviantart,
  SiDiscord,
  SiDouban,
  SiDribbble,
  SiEpicgames,
  SiFacebook,
  SiFigma,
  SiFlickr,
  SiGithub,
  SiGitlab,
  SiGoodreads,
  SiGoogle,
  SiGooglegemini,
  SiHashnode,
  SiHuggingface,
  SiInstagram,
  SiItchdotio,
  SiKaggle,
  SiKakaotalk,
  SiKofi,
  SiLastdotfm,
  SiLetterboxd,
  SiLine,
  SiMaildotru,
  SiMastodon,
  SiMedium,
  SiMisskey,
  SiMyanimelist,
  SiNaver,
  SiNeteasecloudmusic,
  SiNiconico,
  SiNotion,
  SiOdnoklassniki,
  SiOpenai,
  SiOpenrouter,
  SiOrigin,
  SiPatreon,
  SiPinterest,
  SiPixiv,
  SiPlaystation,
  SiProducthunt,
  SiQq,
  SiQqmusic,
  SiReddit,
  SiSinaweibo,
  SiSnapchat,
  SiSoundcloud,
  SiSpotify,
  SiStackoverflow,
  SiSteam,
  SiSubstack,
  SiTelegram,
  SiThreads,
  SiTiktok,
  SiTrakt,
  SiTumblr,
  SiTwitch,
  SiUnsplash,
  SiVk,
  SiWechat,
  SiWhatsapp,
  SiWordpress,
  SiX,
  SiXiaohongshu,
  SiYcombinator,
  SiYoutube,
  SiZhihu,
}

// 导出 Font Awesome
export {
  FaAlipay,
  FaAmazon,
  FaAnchor,
  FaArrowDown,
  FaArrowLeft,
  FaArrowRight,
  FaArrowUp,
  FaBars,
  FaBell,
  FaBellSlash,
  FaBlog,
  FaBolt,
  FaBook,
  FaBrain,
  FaBroadcastTower,
  FaBug,
  FaCalendar,
  FaCalendarAlt,
  FaCamera,
  FaChartBar,
  FaChartLine,
  FaChartPie,
  FaCheck,
  FaCheckCircle,
  FaChevronDown,
  FaChevronLeft,
  FaChevronRight,
  FaChevronUp,
  FaClipboard,
  FaClock,
  FaCloud,
  FaCode,
  FaCoffee,
  FaCog,
  FaComments,
  FaCompass,
  FaCompress,
  FaCopy,
  FaCrown,
  FaDatabase,
  FaDownload,
  FaEdit,
  FaEllipsisH,
  FaEllipsisV,
  FaEnvelope,
  FaExchangeAlt,
  FaExclamationCircle,
  FaExclamationTriangle,
  FaExpand,
  FaExternalLinkAlt,
  FaEye,
  FaEyeSlash,
  FaFeather,
  FaFileAlt,
  FaFilter,
  FaFire,
  FaFolder,
  FaFolderOpen,
  FaFreeCodeCamp,
  FaGamepad,
  FaGem,
  FaGithub,
  FaGlobe,
  FaGraduationCap,
  FaGripHorizontal,
  FaGripVertical,
  FaHdd,
  FaHeadphones,
  FaHeart,
  FaHistory,
  FaHome,
  FaImage,
  FaImages,
  FaInfoCircle,
  FaKey,
  FaLeaf,
  FaLightbulb,
  FaLink,
  FaLinkedin,
  FaList,
  FaLock,
  FaMagic,
  FaMicrophone,
  FaMoon,
  FaMountain,
  FaMusic,
  FaNewspaper,
  FaPaintBrush,
  FaPalette,
  FaPaperPlane,
  FaPause,
  FaPlay,
  FaPlayCircle,
  FaPlus,
  FaQuestionCircle,
  FaRedo,
  FaRobot,
  FaRocket,
  FaSave,
  FaSearch,
  FaServer,
  FaShoppingCart,
  FaSignInAlt,
  FaSignOutAlt,
  FaSlidersH,
  FaSnowflake,
  FaSort,
  FaSortDown,
  FaSortUp,
  FaSpinner,
  FaStar,
  FaSteam,
  FaSun,
  FaSync,
  FaSyncAlt,
  FaTerminal,
  FaTh,
  FaThLarge,
  FaTimes,
  FaTimesCircle,
  FaTools,
  FaTrash,
  FaTwitter,
  FaUmbrella,
  FaUndo,
  FaUnlink,
  FaUnlock,
  FaUpload,
  FaUser,
  FaUsers,
  FaVideo,
  FaVolumeUp,
  FaWater,
  FaWrench,
  FaXbox,
}
export { FaGrip }

// 导出 Lucide Icons
export {
  LuActivity,
  LuAlertCircle,
  LuAlertTriangle,
  LuAlignJustify,
  LuArrowRight,
  LuArrowUpDown,
  LuBarChart3,
  LuBookOpen,
  LuCalendar,
  LuCheck,
  LuCircleDashed,
  LuCheckCircle,
  // 选择 & 复选框
  LuCheckSquare,
  LuChevronDown,
  // 导航 & 方向
  LuChevronLeft,
  LuChevronRight,
  LuChevronUp,
  LuClipboardList,
  // 时间 & 用户
  LuClock,
  LuCloud,
  LuCloudFog,
  LuCloudRain,
  LuCloudSnow,
  LuCloudSun,
  LuCopy,
  LuCpu,
  LuCrown,
  LuDatabase,
  LuDownload,
  LuDroplets,
  LuEdit3,
  LuExternalLink,
  LuEye,
  LuEyeOff,
  LuFileText,
  LuFolder,
  LuFolderOpen,
  LuGauge,
  LuGitFork,
  LuGlobe,
  LuGripVertical,
  LuHeadphones,
  LuHeart,
  LuImage,
  LuInbox,
  LuInfo,
  LuKey,
  LuKeyboard,
  LuLanguages,
  LuLeaf,
  LuLink,
  LuList,
  LuListMusic,
  LuLoader2,
  LuMars,
  LuLock,
  LuMemoryStick,
  LuMessageCircle,
  LuMessageSquare,
  LuMic,
  LuMinus,
  LuMinusSquare,
  LuMonitor,
  LuMoon,
  LuMoreHorizontal,
  LuMusic,
  LuMusic2,
  LuNewspaper,
  LuPackage,
  LuPalette,
  LuPause,
  LuPin,
  LuPinOff,
  LuPlay,
  LuPlus,
  LuRefreshCw,
  LuReply,
  LuRotateCw,
  LuRss,
  LuScale,
  LuSearch,
  LuSearchX,
  LuSend,
  LuServer,
  LuSettings,
  LuShieldAlert,
  LuShieldCheck,
  LuShoppingCart,
  LuShuffle,
  LuSkipBack,
  LuSkipForward,
  LuSlidersHorizontal,
  LuSortAsc,
  LuSparkles,
  LuSquare,
  LuStar,
  LuStore,
  LuSun,
  LuSunrise,
  LuSunset,
  LuTag,
  LuTransgender,
  LuTrash2,
  LuType,
  LuUpload,
  LuUser,
  LuUsers,
  LuVenus,
  LuVideo,
  LuVolume2,
  LuWind,
  LuWrench,
  LuX,
  LuZap,
  LuZoomIn,
  LuZoomOut,
}

// 别名
export const FaXTwitter = SiX

// 品牌图标别名 - 来自其他图标库

/**
 * Nintendo Switch 品牌图标
 * Simple Icons 已移除此图标，使用 Bootstrap Icons 的 BsNintendoSwitch 替代
 */
export const SiNintendoswitch = withIconA11y(BsNintendoSwitch)

// 图标映射表 - 用于动态查找 (getIconByName)
export const iconMap: Record<string, IconType> = {
  // Simple Icons - 品牌图标
  SiSinaweibo,
  SiX,
  SiFacebook,
  SiInstagram,
  SiYoutube,
  SiTiktok,
  FaLinkedin,
  SiReddit,
  SiDiscord,
  SiTelegram,
  SiWhatsapp,
  SiSnapchat,
  SiPinterest,
  SiTwitch,
  SiSpotify,
  SiApple,
  SiAppstore: MyriadStoreIcon,
  MyriadStoreIcon,
  SiGoogle,
  FaAmazon,
  SiDouban,
  SiZhihu,
  SiQq,
  SiWechat,
  SiBaidu,
  SiXiaohongshu,
  SiGitlab,
  SiBitbucket,
  SiCodepen,
  SiDribbble,
  SiBehance,
  SiDeviantart,
  SiMedium,
  SiSubstack,
  SiPatreon,
  SiKofi,
  SiBuymeacoffee,
  SiMastodon,
  SiThreads,
  SiBluesky,
  SiMisskey,
  SiSoundcloud,
  SiBandcamp,
  SiItchdotio,
  SiPlaystation,
  SiNintendoswitch,
  SiEpicgames,
  SiOrigin,
  SiAnilist,
  SiMyanimelist,
  SiLetterboxd,
  SiTrakt,
  SiGoodreads,
  SiLastdotfm,
  SiPixiv,
  SiArtstation,
  SiFlickr,
  SiUnsplash,
  SiProducthunt,
  SiYcombinator,
  SiStackoverflow,
  SiDevdotto,
  SiHashnode,
  SiKaggle,
  SiHuggingface,
  SiFigma,
  SiNotion,
  SiNiconico,
  SiLine,
  SiKakaotalk,
  SiNaver,
  SiVk,
  SiOdnoklassniki,
  SiTumblr,
  SiWordpress,
  SiBlogger,
  SiGithub,
  SiSteam,
  SiNeteasecloudmusic,
  SiBilibili,
  SiMaildotru,
  // Font Awesome - 通用图标
  FaMusic,
  FaVideo,
  FaBroadcastTower,
  FaComments,
  FaGamepad,
  FaShoppingCart,
  FaNewspaper,
  FaCamera,
  FaBook,
  FaGraduationCap,
  FaGlobe,
  FaHome,
  FaStar,
  FaHeart,
  FaRocket,
  FaBolt,
  FaGem,
  FaCrown,
  FaFeather,
  FaLeaf,
  FaPaperPlane,
  FaCompass,
  FaAnchor,
  FaMoon,
  FaSun,
  FaCloud,
  FaFire,
  FaSnowflake,
  FaUmbrella,
  FaMountain,
  FaWater,
  FaCoffee,
  FaPlayCircle,
  FaBlog,
  FaAlipay,
  FaXbox,
  FaEdit,
  FaSave,
  FaTimes,
  FaPlus,
  FaSteam,
  FaTrash,
  FaCheck,
  FaExternalLinkAlt,
  FaExclamationCircle,
  FaChevronDown,
  FaChevronUp,
  FaExpand,
  FaCompress,
  FaChevronLeft,
  FaChevronRight,
  FaCog,
  FaUser,
  FaUsers,
  FaSearch,
  FaEye,
  FaEyeSlash,
  FaCopy,
  FaDownload,
  FaUpload,
  FaSync,
  FaSyncAlt,
  FaSpinner,
  FaInfoCircle,
  FaQuestionCircle,
  FaExclamationTriangle,
  FaCheckCircle,
  FaTimesCircle,
  FaRobot,
  FaBrain,
  FaHdd,
  FaLightbulb,
  FaTwitter,
  FaGithub,
  FaPlay,
  FaPause,
  // Font Awesome 6
  FaGrip,
  FaFreeCodeCamp,
  FaVolumeUp,
  FaMicrophone,
  FaHeadphones,
  FaClipboard,
  FaExchangeAlt,
  // 别名
  FaXTwitter: SiX,
}

/**
 * 根据名称获取图标组件
 * @param name 图标名称（如 "SiGithub", "FaHeart"）
 * @returns 图标组件或 null
 */
export function getIconByName(name: string): IconType | null {
  return iconMap[name] || null
}

// 自定义 SVG 图标组件

/**
 * RSSHub 品牌图标
 * 官方设计：地球 + 网状图案，代表全球化的 RSS 聚合
 * Simple Icons 中没有此图标，使用自定义 SVG
 */
export function RSSHubIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 17.93c-3.95-.49-7-3.85-7-7.93 0-.62.08-1.21.21-1.79L9 15v1c0 1.1.9 2 2 2v1.93zm6.9-2.54c-.26-.81-1-1.39-1.9-1.39h-1v-3c0-.55-.45-1-1-1H8v-2h2c.55 0 1-.45 1-1V7h2c1.1 0 2-.9 2-2v-.41c2.93 1.19 5 4.06 5 7.41 0 2.08-.8 3.97-2.1 5.39z" />
    </svg>
  )
}

// 为了兼容性，导出 NotionIcon 别名（使用 SiNotion）
export const NotionIcon = SiNotion

// 导出类型
export type { IconType, LucideIcon }
