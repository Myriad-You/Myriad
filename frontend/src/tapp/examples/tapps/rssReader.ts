import type { TappManifest } from '@myriad/tapp-runtime'
import type { ExampleTapp } from './types'
import enUS from '../../../../../tapps/rss-reader/i18n/en-US.json'
import jaJP from '../../../../../tapps/rss-reader/i18n/ja-JP.json'
import zhCN from '../../../../../tapps/rss-reader/i18n/zh-CN.json'
import manifestJson from '../../../../../tapps/rss-reader/manifest.json'
import pageCss from '../../../../../tapps/rss-reader/page.css?raw'
import pageHtml from '../../../../../tapps/rss-reader/page.html?raw'
import pageAdmin from '../../../../../tapps/rss-reader/page/admin.js?raw'
import pageApi from '../../../../../tapps/rss-reader/page/api.js?raw'
import pageApp from '../../../../../tapps/rss-reader/page/app.js?raw'
import pageDom from '../../../../../tapps/rss-reader/page/dom.js?raw'
import pageFormat from '../../../../../tapps/rss-reader/page/format.js?raw'
import pageIndex from '../../../../../tapps/rss-reader/page/index.js?raw'
import pageList from '../../../../../tapps/rss-reader/page/list.js?raw'
import pageManage from '../../../../../tapps/rss-reader/page/manage.js?raw'
import pageModel from '../../../../../tapps/rss-reader/page/model.js?raw'
import pageNav from '../../../../../tapps/rss-reader/page/nav.js?raw'
import pageReader from '../../../../../tapps/rss-reader/page/reader.js?raw'
import pageReading from '../../../../../tapps/rss-reader/page/reading.js?raw'
import pageSanitize from '../../../../../tapps/rss-reader/page/sanitize.js?raw'
import pageView from '../../../../../tapps/rss-reader/page/view.js?raw'

const manifest = manifestJson as TappManifest

/** 商店本地目录里的第一方阅读器。源文件在 `tapps/rss-reader`，这里只负责装进现有安装入口。 */
export const rssReaderTapp: ExampleTapp = {
  manifest,
  code: {
    core: '// 阅读器只有页面层。\n',
    page: pageIndex,
    styles: pageCss,
    pageHtml,
    i18n: {
      'zh-CN': zhCN,
      'en-US': enUS,
      'ja-JP': jaJP,
    },
    packageModules: {
      'page/admin.js': pageAdmin,
      'page/api.js': pageApi,
      'page/app.js': pageApp,
      'page/dom.js': pageDom,
      'page/format.js': pageFormat,
      'page/list.js': pageList,
      'page/manage.js': pageManage,
      'page/model.js': pageModel,
      'page/nav.js': pageNav,
      'page/reader.js': pageReader,
      'page/reading.js': pageReading,
      'page/sanitize.js': pageSanitize,
      'page/view.js': pageView,
    },
  },
  tags: ['official', 'rss', 'reader'],
}
