/**
 * Inline first-paint theme boot. Must stay import-free in the emitted IIFE.
 * `/embed/*` 嵌在博客页里：第一帧之前就打上 `embed-frame`，明暗跟着外面那页
 * （components/embed/embedFrame.tsx 的 `embedScheme` 是同一套判断）。
 */
export function themeBootInlineScript(): string {
  return `(function () {
  try {
    var html = document.documentElement;
    var stored = null;
    try { stored = localStorage.getItem('theme'); } catch (e) {}
    var prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    var embed = typeof location !== 'undefined' && location.pathname.indexOf('/embed/') === 0;
    // 外面那页的底色够暗就是深色；读不到（跨源、透明底）再看它声明的 color-scheme。
    var parentScheme = function () {
      try {
        if (window.parent === window) return null;
        var doc = window.parent.document;
        var view = doc.defaultView;
        var nodes = [doc.body, doc.documentElement];
        for (var i = 0; i < nodes.length; i++) {
          if (!nodes[i]) continue;
          var m = /rgba?\\(([\\d.]+)[,\\s]+([\\d.]+)[,\\s]+([\\d.]+)(?:[,\\s/]+([\\d.]+))?/.exec(view.getComputedStyle(nodes[i]).backgroundColor);
          if (!m || m[4] === '0') continue;
          return 0.2126 * m[1] + 0.7152 * m[2] + 0.0722 * m[3] < 128 ? 'dark' : 'light';
        }
        var declared = view.getComputedStyle(doc.documentElement).colorScheme;
        return declared === 'dark' || declared === 'light' ? declared : null;
      } catch (e) {
        return null;
      }
    };
    if (embed) html.classList.add('embed-frame');
    var theme = embed
      ? parentScheme() || (prefersDark ? 'dark' : 'light')
      : stored === 'light' || stored === 'dark' ? stored : prefersDark ? 'dark' : 'light';
    if (theme === 'dark') {
      html.classList.add('dark');
      html.classList.remove('light');
    } else {
      html.classList.add('light');
      html.classList.remove('dark');
    }
    var updateThemeColor = function () {
      var primaryColor =
        getComputedStyle(document.documentElement).getPropertyValue('--color-primary').trim() || '#94a3b8';
      var metaThemeColor = document.querySelector('meta[name="theme-color"]');
      if (metaThemeColor) {
        metaThemeColor.setAttribute('content', primaryColor);
      }
    };
    updateThemeColor();
    new MutationObserver(updateThemeColor).observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['style'],
    });
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function (e) {
      var current = null;
      try { current = localStorage.getItem('theme'); } catch (err) {}
      if (embed || !current || current === 'auto') {
        var next = (embed && parentScheme()) || (e.matches ? 'dark' : 'light');
        if (next === 'dark') {
          html.classList.add('dark');
          html.classList.remove('light');
        } else {
          html.classList.add('light');
          html.classList.remove('dark');
        }
        updateThemeColor();
      }
    });
  } catch (e) {}
})();`
}
