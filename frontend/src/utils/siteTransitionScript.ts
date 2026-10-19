/**
 * 主站和博客之间的整页切换动效（`styles/site-transition.css`）只在这两边之间放。
 * 主站里别的整页跳转（登录回跳、整页重定向）照旧直接换页。
 *
 * 必须在首帧之前挂上：`pagereveal` 在新页面第一次渲染之前就触发，模块脚本赶不上，
 * 所以作为内联脚本放在文档头里。Must stay import-free in the emitted IIFE.
 */
export function siteTransitionInlineScript(): string {
  return `(function () {
  try {
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
    addEventListener('pageswap', function (e) {
      if (!e.viewTransition) return;
      var to = (e.activation && e.activation.entry && e.activation.entry.url) || clicked;
      var lite = document.documentElement.getAttribute('data-perf-mode') === 'exlight';
      if (lite || !to || !isBlog(to)) e.viewTransition.skipTransition();
    });
    addEventListener('pagereveal', function (e) {
      if (!e.viewTransition) return;
      var activation = window.navigation && window.navigation.activation;
      var from = (activation && activation.from && activation.from.url) || document.referrer;
      if (!from || !isBlog(from)) e.viewTransition.skipTransition();
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
