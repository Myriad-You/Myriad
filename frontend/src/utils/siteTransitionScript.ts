import contract from '../../../shared/nav_island.json' with { type: 'json' }

/** 导航岛主条目的 id 和地址，换页时按地址认出主站要选中哪一项。 */
const NAV_ISLAND_ITEMS: { id: string; path: string }[] = contract.items

/**
 * 主站和博客之间的整页切换动效（`styles/site-transition.css`）只在这两边之间放。
 * 主站里别的整页跳转（登录回跳、整页重定向）照旧直接换页。
 *
 * 必须在首帧之前挂上：`pagereveal` 在新页面第一次渲染之前就触发，模块脚本赶不上，
 * 所以作为内联脚本放在文档头里。Must stay import-free in the emitted IIFE.
 */
export function siteTransitionInlineScript(): string {
  const items = JSON.stringify(NAV_ISLAND_ITEMS.map(({ id, path }) => ({ id, path })))
  return `(function () {
  try {
    var items = ${items};
    var isBlog = function (url) {
      try {
        var u = new URL(url, location.href);
        return u.origin === location.origin && (u.pathname === '/blog' || u.pathname.indexOf('/blog/') === 0);
      } catch (e) {
        return false;
      }
    };
    // 没有 Navigation API 的浏览器拿不到去向，就看最后点的是哪个链接。
    var clicked = null;
    addEventListener('click', function (e) {
      var link = e.target && e.target.closest ? e.target.closest('a[href]') : null;
      clicked = link ? link.href : null;
    }, true);
    // 动画档位由主站模块写到 data-perf-mode 上，新页面第一次渲染前还没写，就看上一页记下的
    // （hooks/useAnimationLevel.ts）。
    var lite = function () {
      try {
        return localStorage.getItem('myriad-perf-mode') === 'exlight';
      } catch (e) {
        return false;
      }
    };
    // 博客的选中底色在换页画面里滑到主站要选中的那一项上，停在那里等主站接手。
    // 位置从旧页的图标上量（myriad-nav-icon-<条目>）：主站的导航岛这时多半还没画出来。
    // 量不到或者主站这一页不在导航岛上，就淡掉。
    var slideMark = function () {
      var root = document.documentElement;
      var path = location.pathname;
      var target = null;
      for (var i = 0; i < items.length; i++) {
        var p = items[i].path;
        if (path === p || (p !== '/' && path.indexOf(p.replace(/\\/$/, '') + '/') === 0)) target = items[i].id;
      }
      var center = function (name) {
        var style = getComputedStyle(root, '::view-transition-group(' + name + ')');
        var width = parseFloat(style.width);
        var height = parseFloat(style.height);
        if (!(width > 0 && height > 0)) return null;
        var matrix = new DOMMatrixReadOnly(style.transform);
        return { x: matrix.e + width / 2, y: matrix.f + height / 2 };
      };
      var to = target && center('myriad-nav-icon-' + target);
      var at = center('myriad-nav-current');
      if (!to || !at) {
        root.animate({ opacity: [1, 0] }, {
          duration: 220,
          easing: 'cubic-bezier(0.4, 0, 0.2, 1)',
          fill: 'forwards',
          pseudoElement: '::view-transition-old(myriad-nav-current)',
        });
        return;
      }
      root.animate({ translate: ['0px 0px', (to.x - at.x) + 'px ' + (to.y - at.y) + 'px'] }, {
        duration: 460,
        easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
        fill: 'forwards',
        pseudoElement: '::view-transition-group(myriad-nav-current)',
      });
    };
    addEventListener('pageswap', function (e) {
      if (!e.viewTransition) return;
      var to = (e.activation && e.activation.entry && e.activation.entry.url) || clicked;
      var lite = document.documentElement.getAttribute('data-perf-mode') === 'exlight';
      if (lite || !to || !isBlog(to)) {
        e.viewTransition.skipTransition();
        return;
      }
      // 拍旧页之前：收起悬停提示和放大，选中的底色交给博客那块滑过去（site-transition.css）。
      document.documentElement.classList.add('to-blog');
    });
    // 从博客按返回键回来时这一页是从往返缓存里原样拿出来的，那个 class 还在。
    addEventListener('pageshow', function () {
      document.documentElement.classList.remove('to-blog');
    });
    addEventListener('pagereveal', function (e) {
      document.documentElement.classList.remove('to-blog');
      if (!e.viewTransition) return;
      var activation = window.navigation && window.navigation.activation;
      var from = (activation && activation.from && activation.from.url) || document.referrer;
      if (!from || !isBlog(from) || lite()) {
        e.viewTransition.skipTransition();
        return;
      }
      // 从博客回来：主站这时多半还在加载页。过渡里博客的导航岛一直留在原处（site-transition.css），
      // 加载页不放转圈和站名；主站淡入完由 pageLoader 结束过渡，换成主站自己的导航岛。
      var root = document.documentElement;
      var transition = e.viewTransition;
      root.classList.add('from-blog');
      window.__myriadBlogReveal = transition;
      var done = function () {
        root.classList.remove('from-blog');
        if (window.__myriadBlogReveal === transition) window.__myriadBlogReveal = null;
      };
      transition.finished.then(done, done);
      // 主站在后台预渲染好了的话自己的导航岛已经在，两边的选中项由浏览器配成一对滑过去。
      if (!document.querySelector('.nav-item__mark') && transition.ready) {
        transition.ready.then(slideMark, function () {});
      }
      // 主站一直没好（断网、出错）也不能一直卡在过渡里。
      setTimeout(function () { transition.skipTransition(); }, 5000);
    });
  } catch (e) {}
})()`
}

/**
 * 鼠标停在去博客的链接上（触屏是按下时）就在后台把博客页准备好，点下去时换页动效直接接上，
 * 不等网络。博客页是静态的，提前打开没有副作用。浏览器在省流量、省电时自己会不做。
 * 博客回主站的那一半由后端在博客页的 `Speculation-Rules` 响应头里给。
 */
export const SITE_SPECULATION_RULES = {
  prerender: [
    {
      where: { or: [{ href_matches: '/blog' }, { href_matches: '/blog/*' }] },
      eagerness: 'moderate',
    },
  ],
} as const
