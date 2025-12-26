/**
 * React 版主布局组件
 * 包含导航栏、背景、全局控制面板
 */

import { useEffect, useState, useCallback, useRef } from 'react';
import { useLocation } from 'react-router-dom';
import { API_URL } from '../config';
import { extractColorsFromImage, applyColorPalette } from '../utils/colorExtractor';
import { useWallpaper } from '../hooks/useWallpaper';
import { wallpaperState } from '../utils/wallpaperState';
import GlobalControlPanel from '../components/GlobalControlPanel';
import { useNotification } from '../contexts/NotificationContext';
import { useAnimationLevel } from '../hooks/useAnimationLevel';
import { useEvocativeWallpaper } from '../hooks/useEvocativeWallpaper';
import { SocialNetworkSettingsModal } from '../components/widgets/SocialNetworkWidget';
import { SiteFooter } from '../components/SiteFooter';
import { invalidateAuthCache, getUserAvatarWithCache } from '../utils/userInfoCache';
import { useAuth } from '../contexts/AuthContext';
import { useI18n } from '../contexts/I18nContext';
import NavigationIsland from '../components/NavigationIsland';
import {
  shouldApplyColorExtraction,
  getColorFromCache,
  saveColorToCache,
} from '../utils/wallpaperColorCache';
import { useIdleEffect, useIdleInterval } from '../hooks/useIdleCallback';
import { useScrollOptimization } from '../hooks/useScrollOptimization';
import { startFpsMonitor, stopFpsMonitor } from '../utils/performance';
import { useSystemSetupCheck } from '../hooks/useSystemSetupCheck';
import './AppLayout.css';

interface AppLayoutProps {
  children: React.ReactNode;
}

export function AppLayout({ children }: AppLayoutProps) {
  const location = useLocation();
  const { isAuthenticated, isAdmin, user, checkAuth: checkAuthFromContext } = useAuth();
  const { t } = useI18n();
  const [userAvatar, setUserAvatar] = useState('');
  const [backendConnected, setBackendConnected] = useState<boolean | null>(null);
  const [hasEverConnected, setHasEverConnected] = useState(false);
  const { notifications } = useNotification();

  // ℹ️ 性能优化: 移动端/低端设备禁用背景动画
  const anim = useAnimationLevel();

  // 🔧 帧率优化：启用滚动优化和 FPS 监控
  useScrollOptimization({ enabled: true });
  useSystemSetupCheck();

  // 启动/停止 FPS 监控
  useEffect(() => {
    startFpsMonitor();
    return () => stopFpsMonitor();
  }, []);

  // 壁纸管理 Hook
  const { loadWallpaper: loadWallpaperFromHook } = useWallpaper();

  // Evocative 壁纸动效配置状态
  const [evocativeParallax, setEvocativeParallax] = useState(true);
  const [evocativeDynamicBlur, setEvocativeDynamicBlur] = useState(false);
  const [evocativeRipple, setEvocativeRipple] = useState(false);
  const [evocativeFps, setEvocativeFps] = useState(30);
  const [evocativeRippleQuality, setEvocativeRippleQuality] = useState(0.85);
  // 壁纸模糊度状态
  const [wallpaperBlur, setWallpaperBlur] = useState(3);

  // 🎨 Evocative 壁纸动效统一 Hook
  // ⚠️ 低性能模式下强制禁用所有动效
  const isLowPerformance = anim.level === 'light' || anim.level === 'none';
  useEvocativeWallpaper('wallpaper', {
    parallax: {
      enabled: evocativeParallax && !isLowPerformance,
      enableGyroscope: true,
      enableMouse: true,
      maxOffset: 8,
      scale: 1.02,
    },
    dynamicBlur: {
      enabled: evocativeDynamicBlur && !isLowPerformance,
      baseBlur: wallpaperBlur,
      unblurZone: 0.4,
      blurZone: 0.6,
    },
    ripple: {
      enabled: evocativeRipple && !isLowPerformance,
    },
    fps: evocativeFps,
    rippleQuality: evocativeRippleQuality,
  });

  // 获取用户头像
  useEffect(() => {
    if (user?.username) {
      getUserAvatarWithCache(user.username).then(setUserAvatar).catch(() => {
        setUserAvatar(`https://ui-avatars.com/api/?name=${user.username}`);
      });
    } else {
      setUserAvatar('');
    }
  }, [user]);

  // 加载壁纸和颜色（使用 Hook）
  const loadWallpaper = useCallback(async () => {
    console.debug('[AppLayout] loadWallpaper starting...');
    const wallpaperResult = await loadWallpaperFromHook();

    if (!wallpaperResult) {
      console.debug('[AppLayout] No wallpaper result from hook');
      return;
    }

    console.debug('[AppLayout] Wallpaper loaded:', wallpaperResult.actualUrl.substring(0, 80));

    // 更新 Evocative 动效配置
    if (wallpaperResult.evocative) {
      setEvocativeParallax(wallpaperResult.evocative.parallax);
      setEvocativeDynamicBlur(wallpaperResult.evocative.dynamicBlur);
      setEvocativeRipple(wallpaperResult.evocative.ripple);
      setEvocativeFps(wallpaperResult.evocative.fps);
      setEvocativeRippleQuality(wallpaperResult.evocative.rippleQuality);
    } else {
      // 向后兼容：使用旧字段
      setEvocativeParallax(wallpaperResult.parallaxEnabled);
    }
    // 更新模糊度配置
    setWallpaperBlur(wallpaperResult.blur);

    if (wallpaperResult) {
      const { actualUrl, verified } = wallpaperResult;

      // 如果URL未通过验证，记录警告但继续尝试
      if (!verified) {
        console.warn('壁纸URL验证失败，尝试使用返回的URL进行颜色提取');
      }

      // 🔒 再次验证：确保当前活跃壁纸与要提取颜色的URL一致
      if (!wallpaperState.isUrlActive(actualUrl)) {
        console.warn('壁纸URL在加载期间已变更，跳过颜色提取');
        return;
      }

      // 先检查缓存
      const cachedColors = getColorFromCache(actualUrl);
      if (cachedColors) {
        // 🔒 应用缓存颜色前再次验证
        if (wallpaperState.isUrlActive(actualUrl)) {
          applyColorPalette(cachedColors);
          console.debug('[AppLayout] Applied cached colors');
        }
        return;
      }

      // 检查是否为有效壁纸（包含一致性验证）
      const checkResult = await shouldApplyColorExtraction(actualUrl);
      if (!checkResult.shouldApply) {
        console.debug('[AppLayout] Color extraction skipped:', checkResult.reason);
        return;
      }

      // 提取颜色
      try {
        console.debug('[AppLayout] Starting color extraction for:', actualUrl.substring(0, 80));
        const colors = await extractColorsFromImage(actualUrl, { context: 'wallpaper' });

        // 🔒 应用颜色前验证壁纸是否仍然一致
        if (wallpaperState.isUrlActive(actualUrl)) {
          applyColorPalette(colors);
          saveColorToCache(actualUrl, colors);
          console.debug('[AppLayout] Color extraction completed and applied');
        } else {
          console.warn('颜色提取完成，但壁纸已变更，放弃应用');
        }
      } catch (error) {
        console.error('颜色提取失败:', error);
      }
    }
  }, [loadWallpaperFromHook]);

  // 设置当前导航项
  const setActiveNav = useCallback(() => {
    const navItems = document.querySelectorAll('.nav-item');
    navItems.forEach(item => {
      const href = item.getAttribute('href');
      const ariaLabel = item.getAttribute('aria-label');
      let isActive = false;

      // 处理链接元素（通过 href 匹配）
      if (href) {
        isActive = href === location.pathname || (location.pathname === '/' && href === '/');
      }
      // 处理按钮元素（通过 aria-label 匹配路径）
      else if (ariaLabel) {
        const labelToPathMap: Record<string, string> = {
          [t.nav.library]: '/library',
          [t.nav.reports]: '/reports',
          [t.nav.backToHome]: '/',
        };
        const targetPath = labelToPathMap[ariaLabel];
        if (targetPath) {
          isActive = location.pathname === targetPath;
        }
      }

      if (isActive) {
        item.setAttribute('aria-current', 'page');
      } else {
        item.removeAttribute('aria-current');
      }
    });
  }, [location, t.nav.library, t.nav.reports, t.nav.backToHome]);


  // 检查后端连接状态 - 使用 useIdleInterval 降低主线程占用
  const checkBackendRef = useRef<() => Promise<void>>();
  checkBackendRef.current = async () => {
    try {
      const response = await fetch(`${API_URL}/health`, {
        method: 'GET',
        signal: AbortSignal.timeout(5000), // 5秒超时
      });
      setBackendConnected(response.ok);
      if (response.ok) {
        setHasEverConnected(true);
      }
    } catch {
      setBackendConnected(false);
    }
  };

  // 首次检查延迟到主线程空闲时执行
  useIdleEffect(() => {
    checkBackendRef.current?.();
  }, [], { timeout: 2000 });

  // 每30秒检查一次，使用空闲回调
  useIdleInterval(() => {
    checkBackendRef.current?.();
  }, 30000, {
    enabled: true,
    pauseWhenHidden: true, // 页面隐藏时暂停
    timeout: 5000,
  });

  // 初始化：加载壁纸（仅首次挂载执行）
  const hasInitializedRef = useRef(false);
  useEffect(() => {
    // 防止重复初始化
    if (hasInitializedRef.current) return;
    hasInitializedRef.current = true;

    console.debug('[AppLayout] Initializing wallpaper load...');
    (async () => {
      try {
        await loadWallpaper();
        console.debug('[AppLayout] Wallpaper load completed');
      } catch (error) {
        console.error('[AppLayout] Wallpaper load failed:', error);
      }
    })();
    // 认证检查现在由 AuthContext 管理，按需触发
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 监听壁纸变化事件（由 GlobalControlPanel 触发）
  useEffect(() => {
    const handleWallpaperChanged = async (e: Event) => {
      const customEvent = e as CustomEvent;
      const newUrl = customEvent.detail?.url;

      if (!newUrl) return;

      console.debug('[AppLayout] wallpaperChanged event received:', newUrl.substring(0, 80));

      // 🔒 验证URL与当前活跃壁纸一致
      if (!wallpaperState.isUrlActive(newUrl)) {
        console.debug('[AppLayout] wallpaperChanged: URL not active, skipping');
        return;
      }

      // 先检查缓存
      const cachedColors = getColorFromCache(newUrl);
      if (cachedColors) {
        // 🔒 应用前再次验证
        if (wallpaperState.isUrlActive(newUrl)) {
          applyColorPalette(cachedColors);
          console.debug('[AppLayout] wallpaperChanged: Applied cached colors');
        }
        return;
      }

      // 检查是否为有效壁纸（包含一致性验证）
      const checkResult = await shouldApplyColorExtraction(newUrl);
      if (!checkResult.shouldApply) {
        console.debug('[AppLayout] wallpaperChanged: Extraction skipped:', checkResult.reason);
        return;
      }

      // 提取颜色
      try {
        const colors = await extractColorsFromImage(newUrl, { context: 'wallpaper' });

        // 🔒 应用颜色前验证壁纸是否仍然一致
        if (wallpaperState.isUrlActive(newUrl)) {
          applyColorPalette(colors);
          saveColorToCache(newUrl, colors);
          console.debug('[AppLayout] wallpaperChanged: Colors extracted and applied');
        }
      } catch (error) {
        console.error('颜色提取失败:', error);
      }
    };

    window.addEventListener('wallpaperChanged', handleWallpaperChanged);
    return () => {
      window.removeEventListener('wallpaperChanged', handleWallpaperChanged);
    };
  }, []);

  // 路由变化时更新导航状态
  useEffect(() => {
    setActiveNav();
    // 注意：不在这里自动设置 showLibraryFilters，由按钮点击触发
    // 防止与 handleEnterFilters 冲突导致重复加载
  }, [location.pathname, setActiveNav]);

  // 监听认证状态变化
  useEffect(() => {
    const handleAuthChange = (e: CustomEvent) => {
      const isAuth = e.detail?.isAuthenticated ?? false;

      if (isAuth) {
        // 登录成功，清除缓存并重新检查认证
        invalidateAuthCache();
        checkAuthFromContext();
      }
      // 退出登录时，AuthContext 会自动更新状态
    };

    window.addEventListener('auth-state-changed', handleAuthChange as EventListener);
    return () => {
      window.removeEventListener('auth-state-changed', handleAuthChange as EventListener);
    };
  }, [checkAuthFromContext]);



  // 导航岛自动隐藏逻辑
  useEffect(() => {
    const navContainer = document.querySelector('.nav-container') as HTMLElement;
    if (!navContainer) return;

    // ===== 状态 =====
    let lastScrollY = window.scrollY;
    let rafId = 0;
    let inactivityTimeoutId = 0;
    let isHovering = false;
    let isNavVisible = true;
    let hiddenByScroll = false; // 是否因滚动而隐藏
    let cachedIsDesktop = window.innerWidth >= 768;
    let cachedWindowHeight = window.innerHeight;

    // ===== 配置 =====
    const INACTIVITY_DELAY = 5000;
    const SCROLL_THRESHOLD = 50;
    const PAGE_TOP_THRESHOLD = 100;
    const EDGE_THRESHOLD = 100;

    // ===== 预计算 CSS 变换 =====
    const TRANSFORM_SHOW_DESKTOP = 'translateY(-50%)';
    const TRANSFORM_SHOW_MOBILE = 'translateX(-50%)';
    const TRANSFORM_HIDE_DESKTOP = 'translateY(-50%) translateX(-20px)';
    const TRANSFORM_HIDE_MOBILE = 'translateX(-50%) translateY(20px)';

    // ===== CSS 样式应用 =====
    const applyVisibility = (visible: boolean) => {
      if (visible) {
        navContainer.style.cssText = `
          opacity: 1;
          transform: ${cachedIsDesktop ? TRANSFORM_SHOW_DESKTOP : TRANSFORM_SHOW_MOBILE};
          pointer-events: auto;
          transition: opacity 0.3s ease, transform 0.3s ease;
        `;
      } else {
        navContainer.style.cssText = `
          opacity: 0;
          transform: ${cachedIsDesktop ? TRANSFORM_HIDE_DESKTOP : TRANSFORM_HIDE_MOBILE};
          pointer-events: none;
          transition: opacity 0.3s ease, transform 0.3s ease;
        `;
      }
    };

    // ===== 核心显示/隐藏函数 =====
    const showNav = () => {
      if (isNavVisible) return;
      isNavVisible = true;
      hiddenByScroll = false;
      applyVisibility(true);
    };

    const hideNav = () => {
      if (!isNavVisible || isHovering) return;
      isNavVisible = false;
      applyVisibility(false);
    };

    // 因滚动隐藏（标记状态）
    const hideNavByScroll = () => {
      if (!isNavVisible || isHovering) return;
      isNavVisible = false;
      hiddenByScroll = true;
      applyVisibility(false);
    };

    // ===== 无操作计时器 =====
    const clearInactivityTimer = () => {
      if (inactivityTimeoutId) {
        clearTimeout(inactivityTimeoutId);
        inactivityTimeoutId = 0;
      }
    };

    const startInactivityTimer = () => {
      clearInactivityTimer();
      // 只有导航岛可见时才启动无操作计时
      if (isNavVisible) {
        inactivityTimeoutId = window.setTimeout(hideNav, INACTIVITY_DELAY);
      }
    };

    // ===== 滚动处理 =====
    const processScroll = () => {
      const currentScrollY = window.scrollY;
      const delta = currentScrollY - lastScrollY;
      const isDown = delta > 0;

      // 向下滚动：隐藏（标记为滚动隐藏）
      if (isDown && delta > SCROLL_THRESHOLD && currentScrollY > PAGE_TOP_THRESHOLD) {
        clearInactivityTimer();
        hideNavByScroll();
      }
      // 向上滚动或页面顶部：显示
      else if (!isDown || currentScrollY < PAGE_TOP_THRESHOLD) {
        showNav();
        startInactivityTimer();
      }

      lastScrollY = currentScrollY;
      rafId = 0;
    };

    const handleScroll = () => {
      if (!rafId) {
        rafId = requestAnimationFrame(processScroll);
      }
    };

    // ===== 鼠标移动处理 =====
    let pendingMouseMove: MouseEvent | null = null;
    let mouseRafId = 0;

    const processMouseMove = () => {
      if (!pendingMouseMove) return;
      const e = pendingMouseMove;
      pendingMouseMove = null;
      mouseRafId = 0;

      const isNearEdge = cachedIsDesktop
        ? e.clientX < EDGE_THRESHOLD
        : e.clientY > cachedWindowHeight - EDGE_THRESHOLD;

      // 靠近边缘时显示（即使因滚动隐藏也显示）
      if (isNearEdge) {
        showNav();
        startInactivityTimer();
      }
    };

    const handleMouseMove = (e: MouseEvent) => {
      pendingMouseMove = e;
      if (!mouseRafId) {
        mouseRafId = requestAnimationFrame(processMouseMove);
      }
    };

    // ===== 交互处理 =====
    const handleInteraction = () => {
      // 只有非滚动隐藏状态才响应交互显示
      // 滚动隐藏需要向上滚动或移到边缘才能恢复
      if (!hiddenByScroll) {
        showNav();
        startInactivityTimer();
      }
    };

    // ===== 导航岛悬停 =====
    const handleNavEnter = () => {
      isHovering = true;
      clearInactivityTimer();
      showNav();
    };

    const handleNavLeave = () => {
      isHovering = false;
      startInactivityTimer();
    };

    // ===== 响应式处理 =====
    const mediaQuery = window.matchMedia('(min-width: 768px)');
    const handleMediaChange = (e: MediaQueryListEvent | MediaQueryList) => {
      cachedIsDesktop = e.matches;
      if (isNavVisible) {
        applyVisibility(true);
      }
    };

    const handleResize = () => {
      cachedWindowHeight = window.innerHeight;
    };

    // ===== 初始化 =====
    applyVisibility(true);

    // ===== 事件注册 =====
    const controller = new AbortController();
    const { signal } = controller;
    const passive = { passive: true, signal };

    window.addEventListener('scroll', handleScroll, passive);
    window.addEventListener('mousemove', handleMouseMove, passive);
    window.addEventListener('keydown', handleInteraction, passive);
    window.addEventListener('click', handleInteraction, passive);
    window.addEventListener('touchstart', handleInteraction, passive);
    window.addEventListener('resize', handleResize, passive);
    navContainer.addEventListener('mouseenter', handleNavEnter, { signal });
    navContainer.addEventListener('mouseleave', handleNavLeave, { signal });
    mediaQuery.addEventListener('change', handleMediaChange, { signal });

    // 初始化媒体查询状态
    handleMediaChange(mediaQuery);
    startInactivityTimer();

    // ===== 清理 =====
    return () => {
      controller.abort();
      if (rafId) cancelAnimationFrame(rafId);
      if (mouseRafId) cancelAnimationFrame(mouseRafId);
      clearInactivityTimer();
    };
  }, []);

  return (
    <>
      {/* 全局控制面板 */}
      <div id="global-control-panel-root">
        <GlobalControlPanel />
      </div>

      {/* 背景 */}
      <div id="bg-container" className="fixed inset-0 -z-10 overflow-hidden">
        <div id="wallpaper" className="absolute inset-0 bg-cover bg-center bg-no-repeat transition-opacity duration-700 ease-in-out"></div>
        <div id="bg-gradient" className="absolute inset-0 bg-gradient-to-b from-transparent from-[35%] via-white/40 via-[55%] to-white/90 to-[85%] transition-opacity duration-500 ease-out"></div>
        {/* ⚠️ 性能优化: 只在标准设备上渲染动画背景
            🔥 使用 GPU 加速的独立合成层，避免 mix-blend-mode 导致的 CPU 回退 */}
        {anim.level === 'standard' && (
          <div className="absolute inset-0 opacity-20 transition-opacity duration-700 bg-animation-container">
            {/* 🔥 移除 mix-blend-multiply，改用 opacity 叠加，确保 GPU 合成 */}
            <div className="absolute top-[40%] left-10 w-96 h-96 bg-green-400/40 rounded-full filter blur-3xl animate-blob-fast bg-blob-element" />
            <div className="absolute top-[40%] right-10 w-96 h-96 bg-pink-400/40 rounded-full filter blur-3xl animate-blob-fast animation-delay-2000 bg-blob-element" />
            <div className="absolute top-[60%] left-1/2 -translate-x-1/2 w-96 h-96 bg-blue-400/35 rounded-full filter blur-3xl animate-blob-fast animation-delay-4000 bg-blob-element" />
          </div>
        )}
        <div className="absolute inset-0 bg-grid-pattern opacity-[0.02]"></div>
      </div>

      {/* 导航栏 - 使用新的 NavigationIsland 组件 */}
      <NavigationIsland />

      {/*
        屏幕角落提示容器 - 统一管理所有固定提示，确保不重叠

        使用说明：
        1. 所有需要显示在屏幕角落的提示都应该添加到这个容器内
        2. 容器使用 flex-col gap-3 自动堆叠提示
        3. 父容器 pointer-events-none，子元素需要 pointer-events-auto
        4. 响应式定位已配置好，自动避开导航岛
      */}
      <div className="fixed z-[100] pointer-events-none
        bottom-6 left-6
        md:bottom-6 md:left-[7.5rem]
        flex flex-col gap-3 max-w-xs">

        {/* 后端未连接提示 - 只在曾经连接过但现在断开时显示 */}
        {backendConnected === false && hasEverConnected && (
          <div className="pointer-events-auto animate-fade-in">
            <div className="glass rounded-xl px-4 py-3 shadow-lg border border-red-200/50 dark:border-red-800/50 bg-red-50/80 dark:bg-red-950/80 backdrop-blur-md">
              <div className="flex items-center gap-3">
                <div className="flex-shrink-0">
                  <div className="w-2 h-2 bg-red-500 rounded-full animate-pulse"></div>
                </div>
                <div>
                  <p className="text-sm font-medium text-red-900 dark:text-red-100">{t.setup.backendDisconnected}</p>
                  <p className="text-xs text-red-700 dark:text-red-300 mt-0.5">{t.setup.reconnecting}</p>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* 全局通知 - 从 NotificationContext 渲染 */}
        {notifications.map(notification => (
          <div key={notification.id} className="pointer-events-auto animate-fade-in">
            <div className={`glass rounded-xl px-4 py-3 shadow-lg border backdrop-blur-md ${
              notification.type === 'loading' ? 'border-gray-200/50 dark:border-neutral-700/50' :
              notification.type === 'error' ? 'border-red-200/50 dark:border-red-800/50 bg-red-50/80 dark:bg-red-950/80' :
              'border-blue-200/50 dark:border-blue-800/50 bg-blue-50/80 dark:bg-blue-950/80'
            }`}>
              <div className="flex items-center gap-3">
                {notification.type === 'loading' && (
                  <div className="w-4 h-4 rounded-full bg-gradient-radial from-indigo-400/30 to-transparent animate-pulse"></div>
                )}
                {notification.type === 'error' && (
                  <div className="flex-shrink-0">
                    <div className="w-2 h-2 bg-red-500 rounded-full"></div>
                  </div>
                )}
                {notification.type === 'info' && (
                  <div className="flex-shrink-0">
                    <div className="w-2 h-2 bg-blue-500 rounded-full"></div>
                  </div>
                )}
                <span className={`text-sm font-medium ${
                  notification.type === 'error' ? 'text-red-900 dark:text-red-100' :
                  notification.type === 'info' ? 'text-blue-900 dark:text-blue-100' :
                  'text-gray-700 dark:text-gray-200'
                }`}>
                  {notification.message}
                </span>
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* 主内容区域 */}
      <main className="relative z-10">
        {children}
      </main>

      {/* 全局设置弹窗 - 整个应用只渲染一次 */}
      <SocialNetworkSettingsModal />

      {/* 站点底部信息 */}
      <SiteFooter isHomePage={location.pathname === '/'} />
    </>
  );
}

export default AppLayout;
