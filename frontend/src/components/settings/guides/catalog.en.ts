import type { SettingGuidesCatalog } from './types'

/**
 * English setting guides (plain language + what connects to what)
 * Structure: what it is → what it affects → where to see / how to verify → pitfalls
 */
export const en: SettingGuidesCatalog = {
  ui: {
    siteUrl: {
      what: 'The public address people use to open your site, like https://your-domain.',
      chain:
        '1) Change it here and save — it applies right away (you don’t need the bottom “Save configuration” button).\n2) After that, every full address the site reports to the outside world uses the new one: where GitHub/Google login sends people back, how other sites recognize you, share links, and so on.\n3) The system also lists “what to re-check after a domain change” — go through that list item by item.',
      frontend:
        'After changing it, try this: sign out, use a third-party login, and see whether you land back on this site cleanly.\nShare links and connections with other sites also follow the new address. Page colors and themes do not change by themselves.',
      notes:
        'When the domain changes, the “return address” registered at login services (GitHub and the like) usually needs updating too, or login will fail.\nCertificates and domain DNS must match what you put here. When unsure, follow the checklist the system shows.',
    },
    siteIdentity: {
      what: 'Public site card (name, blurb, icon) and whether visitors can install the site as a PWA.',
      chain:
        '1) Edit title / description / icon or the PWA switch → save configuration at the bottom.\n2) The card updates the browser tab, login site name, and share previews; with PWA on, the install name follows the title when possible.\n3) Separate from “Site address”: this does not change how people reach you.',
      frontend:
        'Tab name and icon, login site name, share blurb; mobile/desktop “Install app” when PWA is on.\nDev mode does not register a service worker.',
      notes: 'Icon can be a URL or a small local file. PWA needs HTTPS (or localhost).',
    },
    siteTitle: {
      what: 'The site’s display name (the short name people see).',
      chain:
        '1) Change the name → save at the bottom.\n2) Anywhere that shows “what this site is called” updates, such as the tab title.\n3) Does not change the domain or login return addresses.',
      frontend: 'Browser tab title, site name on the login page, and similar labels. After save and refresh you should see it immediately.',
      notes: 'Keep it short and easy to recognize; don’t write a long sentence.',
    },
    siteDescription: {
      what: 'One or two sentences explaining what this site is for.',
      chain:
        '1) Fill it in → save at the bottom.\n2) Share links and some previews will carry this text.\n3) Does not change navigation or permissions.',
      frontend: 'The short blurb that sometimes appears when you paste the URL into chat or social apps.',
      notes: 'If you write too much, outside apps only show the first lines anyway — one or two sentences is enough.',
    },
    siteFavicon: {
      what: 'The small icon on the browser tab.',
      chain:
        '1) Paste a link or upload → save at the bottom.\n2) The browser shows the new icon next time (sometimes it still shows the old one first).\n3) Unrelated to wallpaper or theme colors.',
      frontend: 'The little image on the tab corner and in bookmarks. After changing it, hard-refresh or try a private window.',
      notes: 'For local upload, pick a small image; very large files may fail to upload.',
    },
    siteSeo: {
      what: 'Help search engines and social previews understand your site: keywords, share image, and whether to allow indexing.',
      chain:
        '1) Edit SEO fields here → save config at the bottom.\n2) After save, this page updates meta (keywords / robots / Open Graph) right away.\n3) Works with Site identity above: title and description also feed share cards.',
      frontend:
        'meta keywords, robots, og:*, and twitter:* in page source.\nLink preview image and text in chat apps.',
      notes:
        'Keywords barely affect Google ranking — don’t stuff them. Prefer a publicly reachable image URL for shares; pure data: images are invisible to most crawlers.',
    },
    siteKeywords: {
      what: 'Keywords written into the page meta keywords tag.',
      chain:
        '1) Enter (comma-separated) → save at the bottom.\n2) Writes <meta name="keywords">.\n3) Does not change navigation or permissions.',
      frontend: 'The keywords meta tag in the page head.',
      notes: 'Modern engines almost ignore keywords; optional. Empty is fine.',
    },
    siteOgImage: {
      what: 'The large preview image on link cards (Open Graph / Twitter).',
      chain:
        '1) URL or upload → save at the bottom.\n2) Writes og:image and twitter:image.\n3) If empty, falls back toward the site favicon when possible.',
      frontend: 'Link previews in social apps; validators like Facebook Sharing Debugger can confirm.',
      notes:
        'Landscape ≥1200×630 works best. Uploaded data URLs rarely work for crawlers — use a public image URL in production.',
    },
    googleSiteVerification: {
      what: 'HTML-tag ownership check for Google Search Console.',
      chain:
        '1) Add the property in Search Console → choose HTML tag.\n2) Paste the content value or the whole meta tag here → save at the bottom.\n3) Google fetches the homepage, checks the meta, then you can submit /sitemap.xml.',
      frontend:
        '<meta name="google-site-verification"> on crawler HTML shells and the SPA head. Google’s verification fetch uses the crawler shell.',
      notes:
        'Do not use HTML-file verification (this app is an SPA and will not serve google*.html at the site root). DNS TXT verification does not use this field.',
    },
    siteVisibilityPolicy: {
      what: 'Four-level policy for search engines and AI discovery/citation.',
      chain:
        '1) Pick a level → save config.\n2) Backend updates robots.txt / sitemap / llms.txt and syncs noindex.\n3) Public sites: prefer Allow AI citations.',
      frontend: 'Differences on GET /robots.txt, /sitemap.xml, /llms.txt; page robots meta.',
      notes: 'Not all AI bots honor robots; check CDN AI bot settings (e.g. Cloudflare) separately.',
    },
    siteAiIntro: {
      what: 'AI-facing site blurb written into /llms.txt.',
      chain: '1) Write or AI-generate → save.\n2) Served when policy is Allow AI citations or Fully open.',
      frontend: 'Summary block in public /llms.txt.',
      notes: 'Prefer facts and scope over marketing fluff.',
    },
    siteAiGenerate: {
      what: 'Per-field “AI generate” title tags for description / keywords / AI intro.',
      chain: '1) Set site title → click the tag next to a field → review → save config.',
      frontend: 'Title-adjacent SettingTitleTag; POST /api/seo/generate-copy with a single field.',
      notes: 'AI generate opens an optional hint dialog, then POSTs a single field.',
    },
    pwaEnabled: {
      what: 'Whether PWA is enabled (service worker + installable manifest), under “Site identity & app” with the title and icon.',
      chain:
        '1) When on, production registers /sw.js and keeps the web app manifest.\n2) Applies right after save — no full hard reload required.',
      frontend: 'Browser “Apps” / install prompts; DevTools → Application → Service Workers.',
      notes: 'Defaults on. Install prompts need HTTPS (or localhost).',
    },
    siteFooter: {
      what: 'The bar at the very bottom of the page: filing numbers, cloud-provider badges, and similar.',
      chain:
        '1) Put filing numbers and sponsor badges here → save at the bottom.\n2) The bottom of every page on the site updates together.\n3) Display only for visitors — it does not really change your acceleration routes or server network.',
      frontend: 'Scroll to the bottom of any page to see it. If everything is empty, that block may not show at all.',
      notes: 'If you have no filing number, leave it blank — don’t invent someone else’s.',
    },
    siteIcp: {
      what: 'ICP filing number from the regulator (fill only if you have one).',
      chain:
        '1) Enter the number → save at the bottom.\n2) The footer gains a filing line; empty means hidden.\n3) Unrelated to whether the site can be visited — compliance display only.',
      frontend: 'Filing text in the footer. Visible after save and refresh.',
      notes: 'Leave empty if you have none.',
    },
    siteGongan: {
      what: 'Public-security filing number (fill only if you have one).',
      chain:
        '1) Enter the number → save → it shows in the footer.\n2) Empty means hidden.\n3) Can appear together with ICP, or only one of them.',
      frontend: 'The public-security filing line in the footer.',
      notes: 'Leave empty if you have none.',
    },
    cloudSponsors: {
      what: 'Whether the footer shows small logos for certain cloud providers.',
      chain:
        '1) Check the ones you want → save at the bottom.\n2) The footer gains a row of small icons.\n3) Unrelated to whether you actually use those services — pure display.',
      frontend: 'Sponsor / cloud badges in the footer. Unchecked means no badge row.',
      notes: 'You can select none; don’t treat this as “that cloud service is already set up.”',
    },
    siteFooterCustom: {
      what: 'Fully custom icon+text footer blocks — at most 2.',
      chain:
        '1) Add item → text required, icon/url optional → save at the bottom.\n2) Home shows icon+text; other pages and mobile show icon only (tooltip text), same collapse as filing/cloud badges.\n3) Only items with non-empty text appear.',
      frontend: 'Bottom-right footer; soft-refreshes after save.',
      notes: 'Empty-text items are hidden. Links allow http(s), relative paths, mailto — not javascript:.',
    },
    backgroundAndTheme: {
      what: 'Site-wide background, background blur, and similar atmosphere settings.',
      chain:
        '1) Change wallpaper / blur → save at the bottom.\n2) The atmosphere image behind the whole site updates together.\n3) Pairs with “Immersive effects” below: the image is set here; motion on/off is in that group.',
      frontend:
        'Wallpaper behind any page; wallpaper-related entries in the control panel.\nAfter save, switch to the home page — easiest place to notice the change.',
      notes: 'A busy or very blurry image makes text hard to read. If the site stutters, turn a few items off in the effects group first.',
    },
    wallpaper: {
      what: 'Background image (direct URL, 302 redirect, or JSON image API with url/image).',
      chain:
        '1) Paste a direct image URL or common JSON image host → save.\n2) Used as wallpaper across the site.\n3) Parallax and click ripples also act on this image — if the image isn’t set, motion has nothing to ride on.',
      frontend: 'Atmosphere image behind almost every page. Open a fresh home page side by side to compare.',
      notes: 'Smaller images run more smoothly; a broken link shows blank or a broken image. JSON needs fields like url/image/img.',
    },
    wallpaperBlur: {
      what: 'How soft versus sharp the background is.',
      chain:
        '1) Drag the value → save.\n2) Softer background usually makes cards in front easier to read.\n3) Can stack with “dynamic blur” motion — don’t max both or everything goes mushy.',
      frontend: 'Overall background softness. After changing it, check on the home page whether text on cards is clear.',
      notes: 'Very high values strain the computer; if text looks soft, lower the number.',
    },
    evocative: {
      what: 'Make a still wallpaper feel alive: follow the pointer, click ripples, and so on.',
      chain:
        '1) Toggle items here and adjust frame rate / quality → save.\n2) Only changes how pages with wallpaper move — not library or report content.\n3) The wallpaper itself is set under “Background” above; on weaker machines the system may auto-reduce effects a bit.',
      frontend: 'Home and other pages with a background. Move the mouse or click the background to feel it.',
      notes: 'If it stutters: turn off ripples first, or set frame rate to 30. Turning all motion off does not break normal use.',
    },
    evocativeEffects: {
      what: 'Separate on/off for parallax, dynamic blur, and click ripples.',
      chain:
        '1) Only the items you enable actually run.\n2) Together with wallpaper, frame rate, and ripple quality they make the motion experience.\n3) When hunting lag, turn them off one by one to find the culprit quickly.',
      frontend: 'Whether the background moves with the mouse, and whether a click makes a water ripple.',
      notes: 'All three off = still wallpaper (static blur can still stay on).',
    },
    evocativeFps: {
      what: 'How smooth motion should be: 30 saves a little power, 60 feels smoother.',
      chain:
        '1) Choose 30 or 60 → save.\n2) Only affects how fast animations refresh.\n3) Does not change business data; wallpaper and effect switches stay in other items.',
      frontend: 'How smooth wallpaper animation feels. On a laptop, try both and compare.',
      notes: 'Prefer 30 on battery or when visiting from a phone.',
    },
    evocativeRippleQuality: {
      what: 'How finely click water ripples are drawn.',
      chain:
        '1) Adjust the percentage → save.\n2) Only matters when “ripples” are on.\n3) Higher looks better and costs a bit more performance.',
      frontend: 'Ripple edges when you click the background. With ripples off, changing this shows nothing.',
      notes: 'Around 85% is usually enough; lower on weaker devices.',
    },
  },

  modules: {
    visibility: {
      what: 'Which feature doors each audience can see: everyone, signed-in users, or admins only.',
      chain:
        '1) Change a module’s visibility → save the related settings on this page.\n2) The nav bar hides or shows entries by role; people without access can’t get in even by typing the URL.\n3) This is not the same as “Permissions”: here = “can you see the door”; Permissions = “what you can do once inside.”\n4) The assistant entry still needs permissions and quota together before someone can really use it.',
      frontend:
        'Library, reading, reports, apps, assistant, and similar entries on the bottom or side nav.\nUse a private window or another account to confirm what ordinary people still see.',
      notes: 'Admins can usually still open system configuration. Remember to save, and verify with a non-admin account.',
    },
    visibilityItem: {
      what: 'Visibility range for one feature only.',
      chain:
        '1) Only this item changes; other modules stay as they are.\n2) The matching nav button and open access change together.\n3) Set to “admins only” and even normal signed-in users can’t enter.',
      frontend: 'That one nav button and page. Clicking with a normal account is the clearest check.',
      notes: 'To hide from guests but open for signed-in users, choose “signed-in users.”',
    },
    library: {
      what: 'How library cards are laid out, and which platforms’ items show in each category.',
      chain:
        '1) “Card layout” above chooses list or infinite canvas; low-end devices fall back to list automatically.\n2) Connect accounts under “Data platforms” and sync first — only then do sources and counts appear here.\n3) Source checks are display filtering only: unchecked platforms don’t show in the library, but synced data usually still sits on the server.\n4) Report pages also use platform data, but how reports expire or regenerate is under the “Report” group below.\n5) Separate from nav “is the library visible”: visibility lets you enter; inside, layout and sources are set here.\n6) Layout and sources save together with this page into library preferences.',
      frontend:
        'Library list/canvas mode, cards under each category, and the shown / before-filter counts.\nAfter changing, open the library and compare.',
      notes:
        'If there’s no data, sync platforms first. Selecting nothing for a category can leave the list empty. Canvas is heavier — switch back to list if it feels laggy.',
    },
    libraryLayout: {
      what: 'Whether library cards use a regular list or an infinite canvas extending from the page center.',
      chain:
        '1) List = classic scrollable cards; canvas = free drag/zoom from the center.\n2) Low-end devices fall back to list automatically even if canvas is saved.\n3) Independent of which platform sources are checked under each category below.\n4) Saves with library preferences; open the library to compare.',
      frontend: 'Overall arrangement on the library page. Weaker devices may still show the list.',
      notes: 'Canvas costs more GPU/CPU; if interaction feels sticky, switch back to list.',
    },
    libraryType: {
      what: 'Which sources are checked for one category (for example games or anime).',
      chain:
        '1) Filters only this category.\n2) Upstream is still each platform’s sync result.\n3) Other categories are independent and don’t affect each other.',
      frontend: 'Entries under that category tab. After changing checks, refresh the category to look.',
      notes: 'Checking none may empty this category.',
    },
    report: {
      what: 'How long a report stays valid, and whether to auto-make a new one after it expires.',
      chain:
        '1) Report content depends on: platform data already synced + “AI configuration” working + permissions/quota allowing it.\n2) With “expiry” on, reports past the day count are marked expired; with “auto regenerate” also on, they rebuild in the background when due (uses AI quota).\n3) Hiding the “Reports” entry in nav only makes it hard to click in — old reports usually remain.\n4) On failure, tasks or notifications may show a hint.',
      frontend:
        'Expiry cues on the report page, and whether content looks updated.\nOpen the report page for status, or refresh after the automatic job runs.',
      notes: 'Auto regenerate costs quota; turn on expiry first, then auto regenerate. If AI isn’t set up, it keeps failing.',
    },
    reportExpiry: {
      what: 'Whether reports are allowed to “expire.”',
      chain:
        '1) Off = the day count and auto regenerate below basically do nothing.\n2) On = expiry is judged by the day count.\n3) Does not rewrite AI or platform settings — only “is this report considered old.”',
      frontend: 'Whether reports show an expired state. After flipping the switch, open the report page to confirm.',
      notes: 'With expiry off, auto regenerate is usually also unavailable.',
    },
    reportAutoRegen: {
      what: 'Automatically generate a new report after expiry.',
      chain:
        '1) “Expiry” must be on first.\n2) When due → background queue rebuilds → needs AI and platform data both healthy.\n3) On success, the report page shows new content; on failure it may stay on the old report with an error hint.',
      frontend: 'Report content becomes new after the job finishes. Watch notifications for related task messages.',
      notes: 'Fails if AI isn’t set up or quota is used up; don’t expect “click and it’s done instantly.”',
    },
    reportExpiryDays: {
      what: 'How many days a report stays valid.',
      chain:
        '1) Counted from the day it was generated.\n2) Past the days with expiry on → marked expired; with auto regenerate on → may trigger a rebuild.\n3) Changing the number does not rebuild immediately — it only affects how future checks work.',
      frontend: 'Expiry-related cues and timing. After changing, compare with the report’s generation time.',
      notes: 'Usually 1–365 days; when you leave the field it snaps into a valid range.',
    },
    music: {
      what: 'The site’s built-in music player: on or off, and which playlist source.',
      chain:
        '1) Turn the switch on, pick a platform, fill the playlist number → save at the bottom.\n2) The player fetches the list from that music service; the control-panel player and music widgets share the same setup.\n3) Turning the switch off only hides the player — playlist settings stay.\n4) “Clear cache” only forgets the list remembered on this device, not the number you typed.\n5) If the server needs a proxy for the open internet (Advanced settings), that can affect whether songs load.',
      frontend:
        'Player in the control panel, and music-related widgets.\nOnce on, you should see a list or an error message.',
      notes: 'Playlist number must match the platform you chose; private playlists often won’t load. Change the number when you switch platforms.',
    },
    musicPlatform: {
      what: 'Whether the playlist comes from Netease Cloud Music or QQ Music.',
      chain:
        '1) Switch platform → save.\n2) Use that service’s playlist number; old cache may no longer match.\n3) Works only together with the player master switch and the playlist number.',
      frontend: 'Which service tracks and covers in the playlist come from. After switching, clear cache once, then listen again.',
      notes: 'When you switch platforms, always update the playlist number below.',
    },
    musicPlaylist: {
      what: 'Playlist number (usually the digits in the share link).',
      chain:
        '1) Fill the number + player on + save.\n2) Fetches the list from the matching platform.\n3) Only on success do you get clickable songs; failure shows that the list couldn’t load.',
      frontend: 'Playlist and current track. Open the player from the control panel to check.',
      notes: 'Private playlists and wrong IDs both fail; open the playlist in a browser first to confirm you can access it.',
    },
    musicCache: {
      what: 'Forget the playlist this device remembered, and download the list again next time.',
      chain:
        '1) Click clear.\n2) Does not change switches or IDs.\n3) Next open of the player re-requests the list (may be a bit slower, but may be fresher).',
      frontend: 'List content may refresh. Clear, then open the player right away to try.',
      notes: 'Very useful when you changed the playlist number but still hear the old list.',
    },
    hitokoto: {
      what: 'Where the “daily line” on the home page / widgets comes from.',
      chain:
        '1) Pick a source (or custom) → save.\n2) The quote widget fetches a line on its own — not AI, and not data platforms.\n3) For custom, the other site must allow your site to read it, or the browser blocks the request.',
      frontend: 'Quote / short-line widgets and similar welcome displays. After changing, refresh the home page.',
      notes: 'Custom needs the right address and field names; unrelated to assistant chat.',
    },
    hitokotoSource: {
      what: 'Built-in Chinese / English / Japanese sources, or your own endpoint.',
      chain:
        '1) Built-in → uses that public service directly.\n2) Custom → the address and field options below appear.\n3) After save, the widget fetches from the new source.',
      frontend: 'Language and style of the line change. Refresh a page that shows quotes to confirm.',
      notes: 'Custom must also have an address filled, or nothing loads.',
    },
    hitokotoCustomUrl: {
      what: 'Full web address of your custom quote source.',
      chain:
        '1) Used only when the source is “custom.”\n2) When the page opens, content is fetched from this address.\n3) Then the field names below pick out the text and author.',
      frontend: 'Which line is shown. Open the address in a browser to see whether data is there.',
      notes: 'Prefer addresses starting with https; an address only on your own computer is unreachable for outside visitors.',
    },
    hitokotoTextField: {
      what: 'In the returned data, which field is the quote body.',
      chain:
        '1) After a custom source returns data, the text is taken by this name.\n2) Wrong name → blank or odd display.\n3) Independent from the author field.',
      frontend: 'Main quote content. If empty, match the field names in the returned data and fix them.',
      notes: 'Common names include hitokoto; follow the other side’s documentation.',
    },
    hitokotoAuthorField: {
      what: 'Which field is the author or source.',
      chain:
        '1) If present, shown under the quote.\n2) Can be left empty to show only the body.\n3) Only meaningful for custom sources.',
      frontend: 'The attribution line.',
      notes: 'If the other side has no author field, leave empty.',
    },
    island: {
      what: 'What the collapsed top-right control island cycles through.',
      chain:
        '1) Tick greeting, weather, quote, music, Tapp → save at the bottom.\n2) After save, the island updates without a full reload.\n3) Notifications on the island stay under Notification settings, not this group.\n4) If the music player is off, ticking music here still has nothing to show.',
      frontend: 'The capsule control island at the top-right of every page.',
      notes: 'If nothing is ticked the island stays, empty, and still opens the control panel.',
    },
    islandContent: {
      what: 'Each item in this group: show it on the island carousel or not.',
      chain:
        '1) Greeting, weather, quote, music, and Tapp are independent.\n2) Weather still needs a location; quote still needs a working source; music still needs the player on with a track.\n3) Tapp still has to write dynamic content itself.\n4) Notifications are not in this group.',
      frontend: 'The lines currently cycling on the control island.',
      notes: 'This is one site-wide owner setting; visitors see the same island.',
    },
  },

  platforms: {
    list: {
      what: 'Data & stats overview: connected platforms, visitor stats (pages / events / referrers), and AI usage stats.',
      chain:
        '1) Under Connected platforms, link GitHub, Steam, Bilibili, etc. and set auto-refresh.\n2) Under Visitor stats, review KPIs, trends, and page/event/referrer sections.\n3) Under AI usage stats, review full-site text, image, and speech usage.\n4) Card switches only control report-page visibility; auto-refresh syncs configured platforms.',
      frontend:
        'Settings → Data & stats. Platform data feeds library and reports; visitor and AI ledger aggregates are admin-only.',
      notes: 'Visitor stats do not store raw IPs; keep platform secrets private. AI usage is admin-only.',
    },
    visitorStats: {
      what: 'Site-wide views, unique visitors, top countries, plus page / event / referrer sections in the same subcategory.',
      chain:
        '1) Title-row switch controls collection (on by default; when off the server rejects new beacons).\n2) The client beacons on page open (batched / idle).\n3) The server aggregates PV/UV by server-local calendar day; egress IP may resolve top countries.\n4) Switch 7/14/30 days for trends shared by all sections below.\n5) Nested sections each have their own option guide: pages, events, referrers.',
      frontend: 'Data & stats → Visitor stats subcategory (KPIs including countries, chart, nested lists).',
      notes: 'Historical data remains readable when collection is off. Admin and site-owner sessions are excluded. Use “Opt out on this device” in this group to stop beacons from this browser only. Same visitor counts once per day for UV (hashed; no raw IP). Country tiles can be empty when geo lookup fails.',
    },
    analyticsOptOut: {
      what: 'Stop this browser from sending visit beacons. Does not turn off site-wide collection or affect other people.',
      chain:
        '1) On: this device stops sending visitor beacons.\n2) If the site-wide collection switch stays on, other visitors are still counted.\n3) The group switch blocks all new beacons; this one only blocks the current browser.\n4) Stored locally — clearing site data or switching browsers resumes reporting.',
      frontend:
        'Data & stats → Visitor stats → “Opt out on this device.” Compare with a private window: private still counts, this browser does not.',
      notes: 'Does not delete history. Admin sessions are already excluded.',
    },
    pageAnalytics: {
      what: 'Traffic by route in the selected range.',
      chain:
        '1) Same source and range as visitor stats.\n2) Aggregated by path.\n3) Dynamic routes collapse to templates (e.g. /tapp/:id).',
      frontend: 'Data & stats → Visitor stats → Page analytics section.',
      notes: 'Unknown paths bucket as Other to limit cardinality.',
    },
    eventAnalytics: {
      what: 'How often product events fire in the selected range, and how many visitors hit them.',
      chain:
        '1) Shares the visitor-stats collection switch, date range, and exclusions (admin/owner not counted).\n2) Count = sum in range; visitors = distinct people who fired the event.\n3) Built-ins cover login/OAuth, music, library, Brew, report stage, Agent, Tapp, friend links, theme/locale/control panel; pass target for per-entity split (tapp id, platform slug, …).\n4) Main row = event total; sub-rows = target breakdown (up to ~20).\n5) Turning collection off stops new writes; history stays readable.',
      frontend: 'Data & stats → Visitor stats → Events section (beside Referrers).',
      notes: 'Keep custom event names stable and short; noisy high-frequency events crowd the ranking. Export/import backups include event aggregates and are sealed with an instance integrity token so hand-edited metrics are rejected.',
    },
    referrerAnalytics: {
      what: 'Which external sites sent traffic here (views by referrer hostname).',
      chain:
        '1) Same range as visitor stats.\n2) Only external Referer hostnames; in-site navigation is ignored.\n3) Missing or unparsable Referer does not appear in this list.\n4) Disabling collection only affects new visits.',
      frontend: 'Data & stats → Visitor stats → Referrers section.',
      notes: 'Private mode, cross-site HTTPS, and in-app browsers often strip referrers. Ranking is by views, not unique visitors.',
    },
    aiUsage: {
      what: 'Full-site AI usage (including admins/owner): daily trend, by user/model/source ranks, with filters.',
      chain:
        '1) Every provider call (text, image, speech) writes tapp_ai_cost_ledger. Tapp / scheduled jobs still settle quota on the governed path; Agent, reports, Agent persona, playground, and speech are labeled; unlabeled calls land as internal.\n2) Admin GET /api/analytics/ai-usage aggregates by the server local calendar day and does not exclude staff.\n3) Bars = calls (left axis), line = tokens (right axis, independent scale); lists by user, model, and source.\n4) Independent of the visitor-stats collection switch.',
      frontend: 'Settings → Data & stats → “AI usage stats” (KPIs, chart, by user / model / source).',
      notes: 'Tokens are often estimates (images by resolution, speech by text/audio length). Panel is admin-only.',
    },
    thirdPartyAnalytics: {
      what: 'Send visits to external analytics (Google Analytics, Umami, …), separate from first-party visitor charts on this page.',
      chain:
        '1) At the bottom of Data & Analytics, fill GA and/or Umami → save.\n2) Visitor browsers load matching scripts and send page_view.\n3) Visitor Stats charts above stay first-party and are not sent to GA/Umami.',
      frontend: 'Settings → Data & Analytics → last group “Third-party Analytics”.',
      notes: 'Both can be empty (no third-party scripts). Enable one or both.',
    },
    gaMeasurementId: {
      what: 'Your Google Analytics 4 Measurement ID — sends visits to your GA property.',
      chain:
        '1) Copy the G-… ID from GA Admin → Data streams.\n2) Paste under Third-party Analytics → save config.\n3) The page loads gtag.js; SPA route changes send page_view.',
      frontend:
        'Network tab should show googletagmanager.com/gtag/js.\nGA Realtime should list matching page_path values.',
      notes:
        'GA4 only (G-…). Admin/owner browsing is not reported. First-party opt-out also skips third-party. Leave empty to disable GA.',
    },
    umamiWebsiteId: {
      what: 'Website ID for this site in Umami (usually a UUID).',
      chain:
        '1) Copy Website ID from Umami site settings.\n2) Save with Script URL below.\n3) Tracker is injected; SPA routes call umami.track.',
      frontend: 'Network tab shows your script URL; Umami realtime should list paths.',
      notes: 'ID alone without script URL does nothing. Same field for Cloud and self-host.',
    },
    umamiScriptUrl: {
      what: 'Full URL of the Umami tracker script.',
      chain:
        '1) Cloud: https://cloud.umami.is/script.js; self-host: https://your-umami/script.js.\n2) Loads after save with Website ID.\n3) data-auto-track is off; SPA router owns page views.',
      frontend: 'A script tag with data-website-id in the page head.',
      notes: 'Host-only values (e.g. https://stats.example.com) get /script.js appended. Prefer https.',
    },
    connected: {
      what: 'Connect external accounts and configure auto-refresh for configured platforms.',
      chain:
        '1) Open a platform card → fill account/keys and save (configured is enough to sync).\n2) The card switch only controls whether that platform appears on the reports page — not refresh or read.\n3) Auto-refresh below schedules sync for all configured platforms.\n4) Drag cards left/right to set report-page card order.',
      frontend:
        'Library entries, report material (when enabled), platform-dependent widgets, and each platform’s data management area.',
      notes: 'Fill required fields before syncing; the switch only toggles report-page visibility.',
    },
    autoRefresh: {
      what: 'How often to auto-pull data for platforms that are already configured.',
      chain:
        '1) Turn on and pick an interval.\n2) Background syncs all configured platforms on schedule (independent of the report-page switch; staggered to avoid bursting APIs).\n3) Library and reports look fresher next time you open them.\n4) With no platforms configured, auto refresh has nothing useful to do.\n5) Too short an interval may get you temporarily limited by the other platform.',
      frontend:
        'Data & stats → Connected platforms → “Refresh frequency”. Check update times on the platform detail page or in the library.',
      notes: 'Manual sync on the detail page still works alongside this. Don’t set production intervals too short.',
    },
    platformCard: {
      what: 'Connection fields plus the report-page visibility switch; open for config and data ops.',
      chain:
        '1) Required fields must be filled before the switch can turn on.\n2) Once configured you can refresh/process on the detail page and join auto-refresh.\n3) The switch only shows/hides the reports-page card; turning it off does not stop data sync.\n4) Drag order affects report-page card order.',
      frontend:
        'Data & stats → Connected platforms list cards; switch on the card, click to open detail and data management.',
      notes: 'Grayed-out switch usually means required fields are incomplete.',
    },
    platformFields: {
      what: 'Username, secret keys, and other connection details.',
      chain:
        '1) Follow on-page steps to copy keys from the official site → paste here → save.\n2) Once configured you can sync without turning on the report-page switch.\n3) Successful sync → downstream modules have data; wrong or expired → sync fails, library/reports may stay old or empty.\n4) Some platforms also relate to third-party login — the steps will say so.',
      frontend:
        'Indirect: whether data appears and whether errors show. After sync, check the library or this platform’s data management area.',
      notes: 'Don’t share secret keys; after rotating a key, come back and update it here.',
    },
    dataPreview: {
      what: 'A snapshot of what this platform already has in the smart-filter cache: account, key numbers, a few samples.',
      chain:
        '1) Loaded once when you open the detail page — not polled in the background.\n2) Comes from smart-filter output, not a live call to the remote platform.\n3) After refresh raw or reprocess, the snapshot reloads once.\n4) Empty until you sync and process.',
      frontend: 'Platform detail “Current data” group.',
      notes: 'Preview only — full lists live in the library.',
    },
    dataManagement: {
      what: 'Data already synced for this platform: pull again, reprocess, or clear local cache.',
      chain:
        '1) Needs connection fields above to work.\n2) Refresh: fetch raw data from the remote platform again.\n3) Reprocess: re-run local parsing on existing raw data without re-fetching.\n4) Clear cache: drop this site’s cached copy for display/speed — not the remote account.\n5) Library and reports pick up results next time they load.',
      frontend: 'Status rows and buttons under “Data management” on the platform detail page.',
      notes: 'Clearing cache cannot undo the local copy; confirm sync works before destructive ops.',
    },
    dataRefresh: {
      what: 'Fetch raw data from the external platform again.',
      chain:
        '1) Click refresh → background task queues.\n2) On success this site has a new raw snapshot; on failure check the task error.\n3) Library/reports can update only after that.\n4) Too frequent pulls may rate-limit you.',
      frontend: '“Raw data” row and refresh button in data management; task progress may show.',
      notes: 'Wrong secrets or network/proxy issues cause failure.',
    },
    dataReprocess: {
      what: 'Re-parse existing raw data on this site without asking the remote again.',
      chain:
        '1) Useful when parsing/mapping changed but remote data did not.\n2) Usually fails or is disabled with no raw data.\n3) Faster and hits the remote less than a full refresh.',
      frontend: 'Reprocess-style button in data management.',
      notes: 'If raw data itself is stale, refresh first instead of only reprocessing.',
    },
    dataClearCache: {
      what: 'Clear this site’s cache copy for the platform.',
      chain:
        '1) Only local cache — not the remote account.\n2) Next display or sync may rebuild cache.\n3) Use when config changed but old lists/cards still show.',
      frontend: 'Clear-cache button; status row shows whether cache exists.',
      notes: 'Asks for confirmation; load may be slower briefly after.',
    },
  },

  notifications: {
    master: {
      what: 'Whether to receive the various alerts this site is set up to send.',
      chain:
        '1) With the master switch off, display styles and individual sources below no longer push to you.\n2) History usually remains — only new pushes stop.\n3) Background work may still happen; you just don’t get a pop-up or red badge.\n4) To receive alerts again: turn master on, then open the matching sources and locations.',
      frontend: 'Notification list, small red badge on nav, corner tips, system notifications. With master off, these should go quiet.',
      notes: 'Options below gray out when it’s off — that’s normal.',
    },
    island: {
      what: 'Whether to show notification-related cues in the nav control area.',
      chain:
        '1) Needs the master switch on.\n2) Also needs the matching “source” allowed, and the nav area checked under locations.\n3) Only affects the nav area — not corner tips or system notifications (those have their own switches).',
      frontend: 'Notification entry or badge on the nav control area. Try a test notification to observe.',
      notes: 'Does nothing while the master switch is off.',
    },
    toast: {
      what: 'Whether short tips pop up in a corner of the page.',
      chain:
        '1) Master switch + this channel on.\n2) Still filtered by “source” and “events” — not everything pops.\n3) Can run together with nav area and system notifications, or only one of them.',
      frontend: 'Brief tip bars that flash in a corner. If too noisy, turn this off or check fewer events.',
      notes: 'For high-volume sources, pair with event filters or you’ll get flooded.',
    },
    browser: {
      what: 'Whether to use the phone/computer’s built-in system notifications.',
      chain:
        '1) Turning on makes the browser ask “Allow notifications?”\n2) Once allowed, they may still pop when the page is in the background.\n3) Still subject to master switch, sources, and event filters.\n4) If you clicked Deny, change permission in browser settings — the site can’t reverse that alone.',
      frontend: 'System notification center. Switch to another tab and see whether they still appear.',
      notes: 'Be careful on shared/public computers.',
    },
    source: {
      what: 'Whether a whole category of sources (assistant, system, federation, and so on) notifies you.',
      chain:
        '1) Turning a source off makes its location and event settings stop applying.\n2) Only affects “alert or not” — not the feature itself (the assistant still runs).\n3) To get alerts: master on → source on → check locations and events.',
      frontend: 'Whether that category still appears in the notification list. Toggle one source and compare.',
      notes: 'To mute one category, turning off the source is more precise than the master switch.',
    },
    locations: {
      what: 'Where this kind of notification appears: list, corner tips, nav area, system notifications, and so on.',
      chain:
        '1) Master switch and this source must be on first.\n2) Checking locations only then matters.\n3) Checking none = this source is effectively muted.',
      frontend: 'The display places you checked. Check only the list if you want less interruption.',
      notes: 'System notifications also need browser permission.',
    },
    events: {
      what: 'Within this category, which concrete events should notify you.',
      chain:
        '1) Unchecked events never enter notifications even if they happen.\n2) If the source is off, everything here is inactive.\n3) Good for fine control when you only want certain successes/failures.',
      frontend: 'Kinds of content in the notification list. Unchecking a few immediately removes a class of messages.',
      notes: 'If unsure, start with everything on, then turn off what gets noisy.',
    },
  },

  ai: {
    llm: {
      what: 'The three text-model tiers: Standard for daily work, Lite to save, Pro for hard jobs.',
      chain:
        '1) Keys live in Providers above; here you only pick a source and model name.\n2) Standard is the default. Pro falls back to Standard when off. Lite jobs stop when Lite is off — they do not fall back.\n3) Agent persona needs Pro. Lite selects motion and writes proactive lines and mood hints.\n4) Permissions and quota still apply.',
      frontend:
        'Assistant chat, reports, and background text jobs use these tiers. Send one line in the assistant to test Standard first.',
      notes: 'Get Standard working before turning on the other two.',
    },
    standard: {
      what: 'The default smart setup for everyday chat, analysis, and generating reports.',
      chain:
        '1) Pick a provider, fill the secret key (and address/model if needed) → save at the bottom.\n2) Assistant chat, report generation, and smart features in some small apps use this set by default.\n3) If “Lite / High quality” is off or unusable, work falls back to this set too.\n4) Also depends on “Permissions” allowing AI use and remaining quota.\n5) If the server needs a proxy for the open internet, set it under “Advanced,” or smart services may not connect.',
      frontend:
        'Whether the assistant panel can answer normally, and whether reports can generate.\nSend one simple message in the assistant first as a connectivity test.',
      notes: 'Get Standard working first, then turn on other tiers. Don’t leak secret keys.',
    },
    lite: {
      what: 'A cheaper setup for simple, repetitive tasks (optional).',
      chain:
        '1) Turn Lite on and configure it (if the secret key is empty it borrows Standard’s).\n2) Tasks marked “lite” prefer this setup.\n3) Off → still uses Standard.\n4) Same permission and quota limits apply.',
      frontend: 'Users usually don’t see a separate page — mainly which setup simple background tasks use.',
      notes: 'Can use a different provider than Standard; safer to make sure Standard works first.',
    },
    agentPersona: {
      what: 'Let Agent speak with the site persona, keep per-person mood and diary, and speak after named events.',
      chain:
        '1) Off by default. Off means chat and tasks only — no event listening, no hidden speech, persona unused.\n2) The switch needs Pro. Chat only injects the persona as system text; it uses the planner/reply tiers.\n3) Lite selects semantic motion and writes proactive lines and mood hints. Without Lite, semantic motion stops and only local blink, breath, hair, and other ambient physics remain; there is no Standard fallback.\n4) On: write the persona on this item’s secondary page. Tags, name, and persona run through Pro.\n5) MEROPE_ENABLED can override the saved switch, but still needs Pro.\n6) Autonomic speech stays hidden; valuable events also notify if the person is not chatting.',
      frontend:
        'Chat uses this persona. Write it on this item’s secondary settings page. The first-level row shows your mood band and current activity. Guests do not see mood.',
      notes:
        'Only the site owner can write the persona. Deleting the persona also clears mood and diary. Heartbeat is never blocked by mood. Speaking aloud is a separate switch on this card, off by default.',
    },
    agentPersonaSpeech: {
      what: 'Whether the persona face speaks chat replies aloud.',
      chain:
        '1) Off by default. Off: replies stay text; the face still mouths the line, with no voice.\n2) On, with speech configured: replies go through the speech pipeline; real audio drives the mouth; text visemes only if TTS fails.\n3) Requires Agent persona on, and a speech provider on the AI page. The switch does not count while persona is off.\n4) Push-to-listen is still manual and separate.\n5) Reader/podcast TTS uses the speech provider, not this switch.',
      frontend: 'Settings → AI → Agent → Speak. Takes effect on the next chat turn after save.',
      notes: 'Voice uses quota. Turning this off does not turn off the persona.',
    },
    qqBot: {
      what: 'Send a QQ private message into site Work and get the result back in the same chat.',
      chain:
        '1) Turn on and save AppID / AppSecret; the backend then connects the open-platform Gateway.\n2) The secret is write-only: reads come back masked, empty clears it, a mask resubmit keeps the stored value.\n3) Off, incomplete credentials, or 401/403 stop the worker. Network jitter reconnects and is not treated as never configured.\n4) Pairing and Work replies are later tickets. This page only decides whether the bot is online.',
      frontend: 'Settings → AI → Agent → QQ bot.',
      notes: 'Do not paste Discord data-platform OAuth here. AppSecret never appears in page plaintext or error text.',
    },
    liteEnable: {
      what: 'Whether to enable the Lite tier.',
      chain:
        '1) Off = Lite jobs stop. They do not fall back to Standard.\n2) On = motion selection and other lite tasks use the Lite setup.\n3) Turning off does not delete what you already filled in.\n4) Agent persona can turn on without Lite; only local ambient physics remain, proactive lines stay short, and mood hints stay off.',
      frontend: 'Indirectly affects which model is used; simple tasks can show cost/speed differences.',
      notes: 'If Standard still doesn’t work, don’t rush to enable Lite.',
    },
    pro: {
      what: 'A stronger, usually costlier tier for harder tasks.',
      chain:
        '1) When on and configured, jobs that need high quality go here.\n2) Off → falls back to Standard.\n3) Usually more expensive; same permission and quota limits.',
      frontend: 'Generation quality may be better. Compare hard questions in the assistant.',
      notes: 'Enable only when needed; not every task needs this tier.',
    },
    proEnable: {
      what: 'Whether to enable the high-quality model tier.',
      chain:
        '1) Off → hard tasks also fall back to Standard.\n2) On → hard tasks may use the high-quality provider/model. Persona setup (tags, name, persona) also uses this tier.\n3) Still limited by permissions and quota; turning off does not delete keys.',
      frontend: 'Quality on hard assistant/report tasks. Ask the same hard question before/after.',
      notes: 'Keep off if cost-sensitive or Standard is enough.',
    },
    image: {
      what: 'Which service is used to generate images.',
      chain:
        '1) Pick a provider, fill secret key and model → save.\n2) Features that support image output can then work.\n3) Permissions must also allow “AI images,” and quota must remain.\n4) Can be a different provider than text chat.',
      frontend: 'Image-related buttons and result images. On failure, check whether the hint is key, permission, or quota.',
      notes: 'Each provider writes model names differently — follow the on-page hints.',
    },
    speech: {
      what: 'Which provider handles read-aloud and recognition: Tencent Cloud, OpenAI, or OpenRouter.',
      chain:
        '1) Pick a provider and save. Keys live in Providers above.\n2) The on-page test speaks a short line and transcribes it back.\n3) Assistant TTS/ASR uses this section.\n4) OpenRouter has no official OpenAI TTS listing; transcription still works.',
      frontend: 'Settings → AI → Speech. Use the on-page test first.',
      notes: 'Official speech: pick OpenAI. Tencent secrets are under Providers.',
    },
    vendors: {
      what: 'One set of credentials per vendor, shared by the whole site.',
      chain:
        '1) Use “Add provider” in the page header to pick a preset. You can add more than one of the same kind.\n2) Each source has its own name, ID, and key.\n3) Text, image, and speech pick from these sources.',
      frontend: 'Settings → AI → “Add provider” in the section header.',
      notes: 'You do not re-enter a key when switching models. New keys here win over older per-feature fields.',
    },
    webSearch: {
      what: 'TinyFish key for Work-mode web search. Search is free; a key is still required.',
      chain:
        '1) Sign up at agent.tinyfish.ai, paste the key here, and save.\n2) Work-mode ai.webSearch and reading-list web fallback use TinyFish Search first (reading lists also Fetch page text).\n3) If this key is empty, search falls back to configured Gemini Google Search.\n4) The key is a host secret: outbound only, never returned to apps.',
      frontend: 'Settings → AI → Web search. After saving, ask a Work-mode question that needs the live web.',
      notes:
        'Visitors do not fill this in. Chat mode does not use web search. Clear the field and save to delete the stored key; saving the mask leaves it unchanged.',
    },
    provider: {
      what: 'Which smart-service company to use.',
      chain:
        '1) Switching providers changes the fields below.\n2) Requests go to that provider.\n3) After switching, re-check that secret key and model name still match.',
      frontend: 'Form fields on the settings page change; wrong setup shows errors when used.',
      notes: 'Don’t switch provider but keep another provider’s model name.',
    },
    apiKey: {
      what: 'The secret password used to connect to the smart service.',
      chain:
        '1) After you fill and save it, only the server backend uses it.\n2) Visitors never see the full secret on the page.\n3) Missing or wrong → chat and generation both fail.\n4) After rotating, the old one stops working immediately.',
      frontend: 'Error messages on failure. Send one test line in the assistant.',
      notes: 'Don’t post it in public groups or put it in public code repositories.',
    },
    baseUrl: {
      what: 'Access address of the smart service (official, or a relay you run).',
      chain:
        '1) Most people leave empty = use the official address.\n2) If you fill a relay, traffic goes through the relay.\n3) Wrong value → all models fail to connect; page look is fine.',
      frontend: 'Page appearance unchanged; failure shows up as AI unavailable.',
      notes: 'If you don’t understand relays, leave empty; a wrong fill is worse than empty.',
    },
    model: {
      what: 'The exact model name to use.',
      chain:
        '1) Decides answer style, strength, and cost.\n2) Must be a name already enabled on your account.\n3) Works only together with provider and secret key.',
      frontend: 'Differences in assistant answer quality and speed.',
      notes: 'Wrong name causes the call to fail; use the name from the provider’s console.',
    },
    heartbeat: {
      what: 'Let Agent run a task on a schedule without you watching.',
      chain:
        '1) Add a task here: name, schedule (preset or cron), and what to do.\n2) When due, Agent runs that instruction in Work mode.\n3) The list can enable, edit, or delete; last result shows on the row.\n4) Independent of persona mood — heartbeat is never blocked by mood.\n5) You must be signed in to read or write these tasks.',
      frontend: 'Settings → AI → Agent → Schedule. Runs also show on the assistant timeline.',
      notes: 'Write a clear action. Invalid cron never fires. Tasks use AI quota.',
    },
    skills: {
      what: 'Ways of working Agent remembers: ones you wrote, or ones it learned or revised while doing tasks.',
      chain:
        '1) After a successful job it may store a skill and reuse it next time.\n2) You can also ask the assistant to learn or edit one.\n3) This page only lists, searches, and deletes — it does not create skill text.\n4) After delete, that skill is no longer used.',
      frontend: 'Settings → AI → Agent → Skills. Badges mark manual / learned / revised.',
      notes: 'Deleting means teaching again. Skills are not permissions and cannot bypass site permissions or quota.',
    },
    memory: {
      what: 'Short facts Agent chose to keep, injected into later chat.',
      chain:
        '1) It writes a row when it thinks a fact is worth keeping.\n2) Search, edit, or delete here.\n3) Edits apply on the next chat turn.\n4) Not the same as persona diary: diary is per person; memory is fact lines.',
      frontend: 'Settings → AI → Agent → Memory. Expand a row to edit the text.',
      notes: 'Do not put secrets in memory. Deleted rows are no longer injected.',
    },
    imageModel: {
      what: 'The exact model name used for image generation.',
      chain:
        '1) Saved together with the image provider.\n2) Must be a name already enabled on that provider.\n3) Still needs image permission and remaining quota.\n4) Can differ from the text model.',
      frontend: 'Settings → AI → Image → Model. On failure, check the name first.',
      notes: 'Each vendor writes names differently — follow the on-page placeholder.',
    },
    speechStt: {
      what: 'The recognition model that turns speech into text.',
      chain:
        '1) Fill after picking a speech provider.\n2) Assistant dictation and the on-page speech test use this.\n3) The name must be enabled at that provider.\n4) Some providers (e.g. MiniMax) do not show this field.',
      frontend: 'Settings → AI → Speech → recognition model. Use the on-page test.',
      notes: 'Not the same name as the read-aloud model. A wrong name fails transcription.',
    },
    speechTts: {
      what: 'The synthesis model that reads text aloud.',
      chain:
        '1) Fill after picking a speech provider.\n2) Assistant TTS, persona speaking, and the on-page test use this.\n3) Persona “Speak” is a separate switch and needs persona on.\n4) OpenRouter often has no official TTS listing.',
      frontend: 'Settings → AI → Speech → TTS model. Use the on-page test first.',
      notes: 'Podcast/reader TTS uses the speech provider itself, not the persona speak switch.',
    },
    speechVoice: {
      what: 'Which voice ID to use for read-aloud.',
      chain:
        '1) Saved with the TTS model.\n2) Must be a voice the current provider supports.\n3) Persona speaking uses this voice too.\n4) Empty falls back to the on-page placeholder default.',
      frontend: 'Settings → AI → Speech → Voice. One test line is enough to hear the difference.',
      notes: 'Voice names are vendor-specific — do not paste an OpenAI voice into Gemini.',
    },
  },

  tripo: {
    connection: {
      what: 'Whether Tripo 3D is enabled, and which API endpoint and key to use.',
      chain:
        '1) Turn enable on, fill the API key (and optional base URL) → save config at the bottom.\n2) Fully independent from “AI configuration” image generation: reference images are only inputs to the 3D pipeline, not the same keys.\n3) Only when enabled and the key is valid may tasks that consume Tripo credits be created.\n4) The key is stored on the server only — never sent to browsers or embedded in model files.\n5) Outbound traffic uses the global proxy under Advanced settings; Tripo does not open a separate network path.',
      frontend:
        'Settings → 3D & Tripo → “Tripo connection” group.\n3D task creation entry points (admin).',
      notes: 'Get the key from platform.tripo3d.ai. Turning enable off greys out fields below but keeps what you already filled.',
    },
    enabled: {
      what: 'Master switch: allow this site to start 3D tasks that spend Tripo credits.',
      chain:
        '1) Off = cannot create credit-consuming tasks; on = can create once key and budget settings are ready.\n2) Does not delete the key or budget values — only blocks task entry.\n3) Unrelated to the AI image switch.',
      frontend: '“Enable Tripo 3D” switch; other inputs in the group become read-only when off.',
      notes: 'On public sites, ensure only admins can change this.',
    },
    apiKey: {
      what: 'Secret key for the Tripo open platform.',
      chain:
        '1) Fill → save at the bottom → stored only on the server.\n2) Visitors and the frontend never see the full key.\n3) Missing or wrong key → task create/poll fails.',
      frontend: 'Password field “Tripo API Key.” On failure, check the task error message.',
      notes: 'Don’t paste into chats or public repos; rotating invalidates the old key immediately.',
    },
    baseUrl: {
      what: 'Root URL of the Tripo API (official default is fine for most people).',
      chain:
        '1) Default is https://openapi.tripo3d.ai/v3.\n2) Change only for a self-hosted relay or if the official host moves.\n3) Wrong value → every 3D task fails to connect.',
      frontend: '“API Base URL” field. Page look is unchanged; failure shows up as failed tasks.',
      notes: 'If you don’t need a relay, keep the default; a wrong fill is worse than the default.',
    },
    webBudget: {
      what: 'Default generation budget for Web/WebGL scenes: which low-poly model, face cap, and max size to persist.',
      chain:
        '1) These are default request parameters; a single call can override them, and results are inspected again after generation.\n2) Aimed at one-character Web scenes: a balance of look and mobile performance.\n3) Too many faces or too large a download slow downloads and frame rate; too low blurs detail.\n4) Unrelated to AI text-to-image resolution/models.',
      frontend: '“Web model budget” group; results become long-lived local assets, not temporary external links.',
      notes: 'Face limit ~4,000–8,000 is recommended (default 5,000). Changing budget does not rebuild old models.',
    },
    model: {
      what: 'Default Tripo low-poly generation model id (for example the P1 series).',
      chain:
        '1) Sets default topology/detail orientation; tasks without an explicit model use this.\n2) Name must match a currently available official model id.\n3) Works with face limit and max download as the default budget.',
      frontend: '“Default low-poly model” text field.',
      notes: 'Default P1-20260311 targets low-poly and cleaner topology; a wrong name fails the call.',
    },
    faceLimit: {
      what: 'Default face-count cap: how dense the mesh may be.',
      chain:
        '1) Sent as the default when requesting generation (callers may override).\n2) Higher = more detail, more VRAM/bandwidth; lower = better for mobile WebGL.\n3) About 4,000–8,000 is recommended; default 5,000 balances look and performance.\n4) Allowed range roughly 50–20,000.',
      frontend: '“Default face limit” number field.',
      notes: 'One-character Web scenes rarely need the absolute maximum; high caps slow phones.',
    },
    maxDownload: {
      what: 'Maximum size (MB) allowed when persisting one generated model.',
      chain:
        '1) After success, the server downloads the model from a temporary URL into local storage; over this cap the save is rejected or fails.\n2) Protects disk from unexpectedly huge files.\n3) Default about 64 MB; adjust for disk and scene needs.',
      frontend: '“Maximum model size” number field.',
      notes: 'Temporary result URLs expire — pair this with task timeout so download finishes in time.',
    },
    taskControl: {
      what: 'How the backend waits on Tripo jobs: how often to poll and how long to wait.',
      chain:
        '1) After create, poll status on the interval until success, failure, or timeout.\n2) On success, download and persist promptly before temporary URLs expire.\n3) Too short an interval may hit provider rate limits; too long makes progress feel stuck.\n4) Independent of connection: with enable/key off, these also have no effect.',
      frontend: '“Task control” group. Users mainly feel “how long until a model appears.”',
      notes: 'Defaults ~2s poll and ~15 minutes timeout match common provider guidance.',
    },
    pollInterval: {
      what: 'Seconds between status queries to Tripo while a job runs.',
      chain:
        '1) Only applies while a task is in progress.\n2) Usually no faster than 2 seconds, to avoid rate limits.\n3) Larger interval = fewer requests, slower progress updates.',
      frontend: '“Polling interval” number field.',
      notes: 'Don’t go aggressive without a clear reason.',
    },
    taskTimeout: {
      what: 'Maximum wait for one job (seconds); after that it is treated as failed.',
      chain:
        '1) Counts from start until success/failure; stops waiting past this many seconds.\n2) Complex jobs may need longer; too short kills valid work, too long holds the queue.\n3) Default about 900 seconds (15 minutes).',
      frontend: '“Task timeout” number field.',
      notes: 'On timeout, check upstream status and logs before retrying.',
    },
  },

  oauth: {
    section: {
      what: 'Let visitors sign in to this site with one click using GitHub, Google, and similar accounts.',
      chain:
        '1) First fill a correct site address under “Basic configuration” so return addresses are right.\n2) Add a login method here and follow the steps to fill the ID and secret from both sides’ dashboards.\n3) Login page shows a button → user signs in at the other site → then returns here.\n4) Open local password registration is controlled under “Users,” not this section.\n5) After changing the site domain, also update the return address registered at the other side’s dashboard.',
      frontend:
        'Third-party buttons on the login / register page.\nA full login attempt in a private window is the best verification.',
      notes: 'If the site address isn’t set up, you’ll see a strong warning — fix basic configuration first. Don’t leak secrets.',
    },
    provider: {
      what: 'The concrete fill-in fields for one third-party login method.',
      chain:
        '1) Fully filled and enabled → the login page shows that brand’s button.\n2) Missing fields often mean you can’t return after clicking.\n3) Return address depends on the site address configuration.',
      frontend: 'That brand’s login button. You can test one provider alone.',
      notes: 'Follow the on-page steps; avoid extra spaces when pasting.',
    },
    callback: {
      what: 'The address this site must receive after a successful login — register it exactly at the login service.',
      chain:
        '1) Built from Site address + this provider’s slug; you usually do not edit it.\n2) The Redirect URI in the provider console must match character-for-character.\n3) After a domain change, update the provider console too.\n4) Discord data sync may show a second callback, separate from login.',
      frontend: 'The callback row inside an expanded sign-in card; copyable. A full private-window login is the check.',
      notes: 'An extra slash or http vs https will fail the return.',
    },
    clientId: {
      what: 'The application ID the login service issued (Client ID).',
      chain:
        '1) Copy it here after creating the app in their console.\n2) Works only together with the secret and callback.\n3) A wrong ID fails on their authorize page.',
      frontend: 'Client ID on the sign-in method card.',
      notes: 'It may appear in the browser; still do not reuse someone else’s.',
    },
    clientSecret: {
      what: 'The application secret the login service issued (Client Secret).',
      chain:
        '1) Only the server uses it to exchange tokens; the page never shows the full value.\n2) Needed with the ID and callback to complete login.\n3) After rotation the old secret dies immediately — update this field.',
      frontend: 'Secret field on the sign-in method card (masked).',
      notes: 'Do not paste into chat or public repos. Saving the mask keeps the stored value.',
    },
    discovery: {
      what: 'OpenID discovery URL used to find authorize and token endpoints automatically.',
      chain:
        '1) Only for OIDC providers (Google and similar). GitHub’s dedicated kind has no such field.\n2) Use the official .well-known/openid-configuration.\n3) A wrong URL blocks the authorize page or token exchange.',
      frontend: 'Discovery URL on an OIDC card.',
      notes: 'Copy from the on-page steps; do not point at a mirror unless you know why.',
    },
    advanced: {
      what: 'This method’s slug, display name, scopes, and icon.',
      chain:
        '1) The slug appears in the callback path — changing it means updating the provider console too.\n2) Display name is the label on the login button.\n3) Scopes decide which profile fields you get; extras may be rejected.\n4) Icon only changes how the button looks.',
      frontend: 'Sign-in method card → expand → Advanced.',
      notes: 'If login already works, do not casually change the slug.',
    },
  },

  permissions: {
    agentPreset: {
      what: 'One-click common combinations of “what the assistant / small apps may do.”',
      chain:
        '1) Clicking a preset bulk-changes many abilities below (chat, analysis, outbound network, schedules, and so on).\n2) After save, small apps and the assistant allow or deny those abilities at runtime.\n3) Whether you “see the assistant” in nav is under module settings; here is “once you see it, can you really use it.”\n4) How many times you can use it is still limited by “quota” below.\n5) After you manually change single items, it may show as “Custom.”',
      frontend:
        'Whether assistant features work, and whether smart/networked actions in small apps are refused.\nLog in as a normal user and try a refused-permission message.',
      notes: 'Be especially careful with guest abilities. Presets don’t randomly change media/theme powers unrelated to the assistant.',
    },
    agentPresetUser: {
      what: 'Which assistant-capability preset signed-in users get.',
      chain:
        '1) Picking a tier bulk-changes user elevated abilities.\n2) Editing Fine-tune below turns it Custom.\n3) Admins are usually not limited by these.\n4) Quota is set separately underneath.',
      frontend: 'Permissions → Agent presets → Users. Check while signed in as a normal account.',
      notes: 'Think through abuse before opening network or scheduler for users.',
    },
    agentPresetGuest: {
      what: 'Which assistant-capability preset unsigned visitors get.',
      chain:
        '1) Unsigned visits are allowed or denied from this tier.\n2) Guests have no full backend assistant session — opening too much invites flooding.\n3) Some abilities (scheduler, federation writes) never appear in the guest preset.\n4) Keep quota clearly below signed-in users.',
      frontend: 'Permissions → Agent presets → Guests. Verify in a signed-out private window.',
      notes: 'Keep this tight in production.',
    },
    fineTune: {
      what: 'On top of a preset, turn individual abilities on or off one by one.',
      chain:
        '1) Change a single item → save.\n2) Matching abilities are checked under the new rules right away.\n3) Common case: the button still shows, but one click says no permission.\n4) Highest admin powers cannot be handed out to others here.',
      frontend: 'Whether each ability truly works. Use “clicked and got no permission” as a checklist.',
      notes: 'If it still looks like a preset after changes, check whether it’s already become a custom mix.',
    },
    userElevated: {
      what: 'Extra things ordinary signed-in users may do.',
      chain:
        '1) After a user signs in, the checks here apply.\n2) Part of the same set as the presets above.\n3) Admins are usually not limited by these.',
      frontend: 'Boundaries of signed-in users’ assistant / small-app abilities.',
      notes: 'Think through abuse risk before giving users “outbound network” and similar powers.',
    },
    guestElevated: {
      what: 'Extra things people who aren’t signed in may do (higher risk).',
      chain:
        '1) Unauthenticated visits are judged by this.\n2) Too open invites flooding of interfaces and AI.\n3) Guests usually don’t get a full backend assistant session.',
      frontend: 'Features guests can still see or click. Verify while signed out in a private window.',
      notes: 'In production, open as little as possible; also keep guest quota low.',
    },
    aiQuota: {
      what: 'How many times users and guests may each use smart features.',
      chain:
        '1) Over the limit → blocked even if models are fully configured.\n2) With permissions and model setup, three gates: all must pass to succeed.\n3) Going over does not auto-switch to a pricier model.',
      frontend: 'Over-quota error messages. Use a normal account near the limit to observe.',
      notes: 'Guest quota should be clearly lower than signed-in users.',
    },
    userQuota: {
      what: 'Usage cap for signed-in users.',
      chain:
        '1) Only constrains signed-in users.\n2) When full, related smart features show failure messages.\n3) When the count resets is decided by server policy.',
      frontend: 'Messages when that role has used up its quota.',
      notes: '0 or extremely low is effectively almost disabled (depending on implementation).',
    },
    guestQuota: {
      what: 'AI usage cap for guests (signed-out).',
      chain:
        '1) First gate against unsigned visitors flooding the site.\n2) Works together with guest permissions.\n3) When full, guest smart features are unavailable.\n4) 0 usually means guests cannot use smart features (see actual errors).',
      frontend: 'Guest trial budget messages; try a private window until over quota.',
      notes: 'Keep clearly lower than signed-in users; prefer low in production.',
    },
  },

  users: {
    section: {
      what: 'Manage who can sign in, who is an admin, and whether visitors may self-register a local username/password.',
      chain:
        '1) Create or adjust accounts here; the top switch controls public local registration.\n2) People sign in with username/password (or third-party methods from “Sign-in methods”).\n3) Admins open system configuration; ordinary users still follow module visibility, permissions, and quotas.\n4) Deleting an account mainly affects login — synced platform display data usually remains.\n5) When public registration is on, guests can open /register to create their own account.',
      frontend: 'Who can sign in, admin menus, and whether the login page shows “Register.” Verify with a new account or private window.',
      notes: 'Only admins open this page; create few admins. Be careful enabling public registration on public sites.',
    },
    create: {
      what: 'Create a new local account; optionally mark as admin.',
      chain:
        '1) Open “Add user,” fill username and password → create.\n2) Usually can sign in right away.\n3) Admin grants configuration powers — use carefully.\n4) Separate path from public self-registration: here an admin creates the account.',
      frontend: 'New row in the list; new account sees different entries after login.',
      notes: 'Use a strong password; double-check before ticking admin.',
    },
    list: {
      what: 'View, search, and filter users; expand a row to adjust roles or login methods.',
      chain:
        '1) Search or filter by role / online status.\n2) Expand for bindings, disabling local password, and so on.\n3) Role changes apply on next login.\n4) Delete and similar actions are hard to undo.',
      frontend: 'User list and filters; after changes have them sign in again.',
      notes: 'Don’t delete the only admin account.',
    },
    allowLocalRegister: {
      what: 'Whether visitors may create a local username/password account on the Register page.',
      chain:
        '1) On: unsigned visitors can open /register and sign up.\n2) Off: local accounts only via admin create on this page.\n3) Not the same as third-party login (GitHub/Google) — that is under Sign-in methods.\n4) Usually takes effect for visitors after config is saved.',
      frontend: '“Allow public local registration” switch at the top of Users; whether login shows “Register.” Try /register in a private window.',
      notes: 'Be careful on public sites (bulk sign-ups). Safer: keep off + admin-created accounts, or trusted third-party login only.',
    },
    privateTappInstall: {
      what: 'When to automatically uninstall a normal user’s personal (private) Tapp installs.',
      chain:
        '1) Default is 14 days inactive: a daily worker prunes by last login/seen; logout itself does not delete.\n2) 7 days is shorter; a custom 1–365 day value also appears in the menu.\n3) “Logout”: wipe that user’s private installs as soon as they sign out.\n4) Does not touch site-wide public installs, nor admin apps in the public namespace.\n5) Cleanup removes the install relationship; the package remains in the store.',
      frontend: 'Top of the Users list, beside public registration.',
      notes: 'On a public site, do not keep personal installs forever. Admin accounts skip logout cleanup.',
    },
  },

  advanced: {
    memorySaver: {
      what: 'Trade concurrent capacity for lower memory use on small hosts (~1 GiB).',
      chain:
        '1) Off (default) = current balanced, bounded budgets: 4 MiB message payload, 8 MiB federation inbox body, 24 MiB authenticated body, and 32 MiB concurrent raw inbox buffering. Upgrading from older releases can therefore change limits.\n2) On = a second notch: 4 MiB inbox inflight, 16 MiB chunk inflight, 8 MiB API cache, DB pool 1–4, 1 Argon2 slot, 32 MiB note video, 16 MiB audio proxy.\n3) Everyday messages still fit; large video or lossless audio should use chunked transfer or turn saver off.\n4) Most knobs apply after Save; DB pool size fully applies after backend reconnect/restart.',
      frontend: 'No page theme change. Very large media or multi-path peaks may reject sooner or show “retry later”.',
      notes: 'Operators can force a profile with MYRIAD_MEMORY_PROFILE=default|saver (env overrides this switch).',
    },
    memorySaverEnable: {
      what: 'Enable memory-saver resource budgets.',
      chain:
        '1) Saves to server config, then reloads runtime concurrency/federation/cache caps.\n2) Federation stays on; budgets are just tighter.\n3) Best for 1 GiB VMs; leave off on larger hosts.',
      frontend: 'Indirect under heavy load. Daily browsing should feel the same.',
      notes: 'Turn off anytime and Save to restore balanced budgets (restart if you care about DB pool size).',
    },
    network: {
      what: 'Whether the server uses a proxy when going to the open internet, and access addresses for a few services.',
      chain:
        '1) These are mainly for “the server itself going online”: syncing foreign platforms, calling smart services, and so on.\n2) Your browser opening this site, or jumping to third-party login, usually does not use this proxy.\n3) Domestic platforms may get slower if forced through a proxy — fill a “don’t use proxy for these” list.\n4) Problems often look like: sync fails, AI unavailable — not a broken page theme.',
      frontend: 'Page look usually unchanged. When something’s wrong, check data sync and whether the assistant errors.',
      notes: 'Don’t change login jump addresses to mirrors; that is not the same as “GitHub data sync address.”',
    },
    proxyEnable: {
      what: 'Whether the server uses a proxy when going out to the internet.',
      chain:
        '1) On → uses the proxy address below for outbound traffic.\n2) Off → direct connection (the address can stay saved unused).\n3) Affects whether platform auto-refresh, AI outbound calls, and similar succeed.',
      frontend: 'Indirect: sync/AI success rate. After enabling the proxy, try syncing a foreign platform once.',
      notes: 'The proxy itself must already work in your network environment.',
    },
    proxyUrl: {
      what: 'Address of the proxy server.',
      chain:
        '1) Only used when the switch above is on.\n2) Wrong fill = the server can’t reach the open internet.\n3) Often provided by a local or company proxy app.',
      frontend: 'No direct interface change. When it fails, sync/AI fail.',
      notes: 'Fill as the proxy app instructs; watch the port number.',
    },
    proxyBypass: {
      what: 'Which addresses should not use the proxy.',
      chain:
        '1) Hosts on the list are reached by the server directly.\n2) Often used for domestic services to avoid a long detour.\n3) Works with the master proxy switch: with the switch off, the list barely matters.',
      frontend: 'None. Smoother domestic platform sync may be thanks to this.',
      notes: 'Separate multiple entries with commas; can include local addresses.',
    },
    geminiBaseUrl: {
      what: 'Root URL for Google Gemini API calls (most people leave empty).',
      chain:
        '1) Used when AI config selects Gemini-related models.\n2) Empty = official URL.\n3) Fill only for your own proxy/mirror; wrong values break Gemini calls.\n4) Separate from per-provider Base URL fields on the AI page — this is the advanced global default.',
      frontend: 'Whether Gemini chat/generation fails to connect. Theme unchanged.',
      notes: 'Leave empty if you don’t use Gemini. Don’t confuse with GitHub API or site URL.',
    },
    githubApiBaseUrl: {
      what: 'Address used when pulling GitHub data (a mirror is allowed).',
      chain:
        '1) Affects whether GitHub platform sync goes smoothly.\n2) When signing in with a GitHub account, the authorize page is still official — this field can’t change that.\n3) Separate from “site address” and login return addresses — three different lines.',
      frontend: 'Whether GitHub-related material syncs successfully.',
      notes: 'Don’t confuse this with login return addresses.',
    },
    backup: {
      what: 'Back up current settings to a file, restore from a file, or reset to factory defaults.',
      chain:
        '1) Export: produces a settings file for you to download.\n2) Import: after review, writes back — many site settings change together.\n3) Reset: everything returns to defaults, hard to undo.\n4) Strongly recommended: export one copy first, then import or reset.',
      frontend: 'After import/reset, theme, modules, platforms, and more may change a lot. Spot-check the whole site afterward.',
      notes: 'Backup files may contain secret keys — don’t forward them freely.',
    },
    exportConfig: {
      what: 'Download a backup of the current settings.',
      chain:
        '1) Click export → get a file.\n2) Does not change the running site.\n3) For moving, backup, or a safety copy before something goes wrong.',
      frontend: 'No immediate change — only an extra file.',
      notes: 'Store it carefully; it contains sensitive information.',
    },
    importConfig: {
      what: 'Restore settings from a backup file.',
      chain:
        '1) Pick a file → confirm the preview → write back.\n2) After success, full page reload (~2s) loads the complete state.\n3) May overwrite your current theme, platforms, AI, and more.',
      frontend:
        'Many options may change together. Import triggers a full page reload; spot-check login, library, and assistant.',
      notes:
        'Wrong files fail; export the current state first if you can. Confirm dialog notes the upcoming full page reload.',
    },
    resetConfig: {
      what: 'Restore everything to defaults.',
      chain:
        '1) Runs after a second confirmation.\n2) Personalized configuration is cleared back to the initial state.\n3) Almost impossible to undo lightly — always back up first.',
      frontend: 'Look and features feel like a fresh install.',
      notes: 'Extremely careful in production; export first.',
    },
    runtimeDiagnostics: {
      what: 'Run a read-only health check of this backend and produce a credential-free diagnostics report.',
      chain:
        '1) Loads once when you open Advanced settings; refresh re-runs the suite.\n2) Checks DB connectivity, storage writability, migrations/schema, process memory, egress location, and frontend/backend version match.\n3) Status is healthy / attention / critical; copy or download JSON for maintainers.\n4) Does not change config, restart services, or write business data.',
      frontend: 'Settings → Advanced → top “Runtime diagnostics” card and check grid.',
      notes: 'Egress location is informational only (single-source/conflict does not raise overall attention). Proxy/firewall may show unavailable without the site being broken. Reports omit passwords and API keys — still avoid pasting internal IPs publicly.',
    },
    frontendCache: {
      what: 'Local advanced tools for troubleshooting (not server config).',
      chain:
        '1) Sits above Backup & restore so you can clear caches before import/reset.\n2) Currently offers force-refresh of frontend caches and similar maintenance.\n3) Does not sign you out or change theme/language.',
      frontend: 'The “Advanced tools” group under Advanced settings.',
      notes: 'Actions only affect this browser.',
    },
    forceRefreshCache: {
      what: 'One-click clear of frontend caches and reload.',
      chain:
        '1) In-place double confirm: first click arms the button, second click runs.\n2) Wipes each cache layer, then full reload.\n3) Good when “I changed config / upgraded, but the browser still uses the old stuff.”',
      frontend: 'Button label switches to confirm state; reverts if not clicked again within ~4s.',
      notes: 'Only this browser; other devices are unaffected.',
    },
    mcp: {
      what: 'Manage external MCP (stdio) tool servers for Agent in the UI: add/edit/remove, enable/disable, and inspect live health.',
      chain:
        '1) Save writes runtime data/agent/mcp_servers.json (often /data/agent/mcp_servers.json in the container) and hot-reloads children (no full site restart).\n2) Enabled servers start command/args; tools surface as mcp.{id}.{tool} for the agent.\n3) The list shows health and tool counts; disabled entries stay on disk but do not start.\n4) Env fields can hold secrets that live only on the server.',
      frontend: 'Settings → Advanced → “MCP tool servers” (admin only). Deep link /config?section=mcp.',
      notes: 'Config path follows the deploy data directory. Commands must be on PATH. Do not commit secrets. A failed save leaves running processes until a successful save.',
    },
    mcpId: {
      what: 'Short id for this MCP server; it appears inside tool names.',
      chain:
        '1) Letters, digits, and . _ - only.\n2) After save, tools are mcp.{id}.{tool}.\n3) Must be unique in this config.\n4) Changing the id is like a new server — old tool names stop working.',
      frontend: 'MCP add/edit form → Server ID.',
      notes: 'Keep it short English, e.g. github.',
    },
    mcpCommand: {
      what: 'Executable used to start this server.',
      chain:
        '1) Something on PATH such as npx, node, or python.\n2) Combined with args into the start line.\n3) If the command is missing, the server will not become healthy.',
      frontend: 'MCP form → Command.',
      notes: 'In containers, confirm the image actually has this binary.',
    },
    mcpArgs: {
      what: 'Arguments after the start command.',
      chain:
        '1) Space-separated; quote segments that contain spaces.\n2) Typical: -y @modelcontextprotocol/server-github.\n3) Hot-reloaded with the command after save.',
      frontend: 'MCP form → Args.',
      notes: 'Wrong package or path fails startup — check health and logs.',
    },
    mcpEnv: {
      what: 'Environment variables for this process, often secrets.',
      chain:
        '1) One KEY=value per line.\n2) Stored only in server config — not git, not returned to apps.\n3) After save the child starts with the new env.',
      frontend: 'MCP form → Environment.',
      notes: 'Do not paste secrets into chat. Empty is fine if the server needs none.',
    },
    mcpMaxRestart: {
      what: 'How many times to auto-start after a crash.',
      chain:
        '1) 0–50. After the cap, no more automatic restarts.\n2) Only matters if crash auto-restart is on.\n3) 0 means never auto-start.',
      frontend: 'MCP form → Max restart attempts.',
      notes: 'Repeated crashes mean a bad command or secret — raising the cap is not a fix.',
    },
    mcpEnabled: {
      what: 'Whether to start this server now.',
      chain:
        '1) On: save starts the child and indexes tools.\n2) Off: does not start; config stays for later.\n3) The list row can toggle this in place.',
      frontend: 'MCP form → Enabled, and the switch on the list row.',
      notes: 'Off is not delete.',
    },
    mcpAutoRestart: {
      what: 'Whether to start again if the child exits unexpectedly.',
      chain:
        '1) On: retries up to max restart attempts.\n2) Off: stays down until you enable or save again.\n3) Separate from Enabled — nothing runs if it is off.',
      frontend: 'MCP form → Restart on crash.',
      notes: 'Safer to enable only for commands you control.',
    },
    mcpTrustAnnotations: {
      what: 'Whether to trust this server’s own “read-only” marks and skip some confirms.',
      chain:
        '1) On: tools it labels read-only may skip the confirm dialog.\n2) Destructive tools always confirm, regardless of this switch.\n3) Annotations are the server’s claim, not a host audit.',
      frontend: 'MCP form → Trust this server’s tool annotations.',
      notes: 'Turn on only for servers you deploy and trust.',
    },
  },

  federation: {
    keys: {
      what: 'This site’s identity and keys when connecting with other sites.',
      chain:
        '1) Used to prove “this post really came from this site,” and to recognize others.\n2) The identity address is bound to the site domain — an unstable domain causes trouble.\n3) After rotating keys, others may fail verification for a short time.',
      frontend: 'Whether following other sites and sending/receiving posts goes smoothly.',
      notes: 'Think carefully before rotating keys; don’t casually click rotate day to day.',
    },
    rotateKeys: {
      what: 'Replace this site’s signing key pair used for federation.',
      chain:
        '1) Generates new keys and tries to notify related sites.\n2) Peers may fail verification briefly until they refresh your public key.\n3) Daily ensure-keys never rotates silently — only after you confirm.\n4) Don’t rotate if the domain is unstable.',
      frontend: 'Rotate button on the federation identity & keys card.',
      notes: 'Rarely needed in production; a mis-click can briefly break federation.',
    },
    policy: {
      what: 'What kind of content from outside sites you’re willing to accept.',
      chain:
        '1) After rules are saved, inbound content is filtered by trust and lists first.\n2) Only what passes may appear in the feed.\n3) To be alerted, you also need the matching source open under “Notifications.”\n4) Too strict → you miss friends’ sites; too loose → spam arrives easily.',
      frontend: 'Whether you can see posts from other sites. Mutual follow with a friend site is a good test.',
      notes: 'Raise trust for familiar sites first, then slowly tighten overall policy.',
    },
    minTrust: {
      what: 'How trusted inbound content must be before you’ll accept it.',
      chain:
        '1) Content from sites below this level is dropped.\n2) Raise friends’ trust under “Known instances.”\n3) Together with allowlist and content filters, this is the inbound threshold.',
      frontend: 'Content from low-trust sites does not appear.',
      notes: 'Add friends first, then raise the bar.',
    },
    allowlist: {
      what: 'Only allow sites on the list to interconnect (if you enable that policy).',
      chain:
        '1) Sites not on the list basically don’t interact.\n2) Can work with auto-discover: discover first, then decide whether to add to the list.\n3) Behavior when the list is empty follows the on-page description.',
      frontend: 'No interaction with sites outside the list.',
      notes: 'When typing domains, avoid extra spaces and typos.',
    },
    autoDiscover: {
      what: 'Whether to automatically remember new sites when you encounter them.',
      chain:
        '1) Once remembered they appear in the “Known instances” list.\n2) Makes it easier later to adjust trust or block.\n3) Still subject to allowlist and minimum trust — not open to everyone.',
      frontend: 'Known instances list grows longer.',
      notes: 'Fine to leave on for public sites; turn off for very private deployments.',
    },
    knownInstances: {
      what: 'Sites already seen: adjust trust, block, search.',
      chain:
        '1) Blocking here rejects that site immediately.\n2) Changing trust affects whether you accept its content later.\n3) Links with the minimum trust in policy.',
      frontend: 'Whether that site’s content / related notifications appear. After blocking, you should stop receiving them at once.',
      notes: 'Blocking affects both ways — confirm the site name before acting.',
    },
    contentFilters: {
      what: 'Another pass of filters on inbound content by type, keywords, and so on.',
      chain:
        '1) Rules take effect immediately.\n2) Content matching rules never enters the feed or notifications.\n3) Another sieve after trust/lists.\n4) Rules written too broadly also block normal content.',
      frontend: 'You don’t see filtered content. Add a test rule first, then remove it.',
      notes: 'Try a small range first; watch for a few days before tightening.',
    },
    deliveryQueue: {
      what: 'Health of the send queue for posts this site delivers to others.',
      chain:
        '1) Content you send is queued for delivery to the other side.\n2) Failures retry or enter a “couldn’t deliver” list.\n3) If the other side never receives you: check the queue + network/proxy + whether they blocked you.\n4) Clearing the queue throws away what’s still unsent.',
      frontend: 'Queue status; whether the other site sees your posts in time.',
      notes: 'Different direction from “limit what others send in” — don’t only tune one side.',
    },
    advanced: {
      what: 'Limits that prevent too many requests in a short time.',
      chain:
        '1) Over the limit temporarily rejects the other side.\n2) More trusted sites can get a higher multiplier.\n3) Defaults are usually enough; tune only when you’re being flooded.',
      frontend: 'The other side may fail to sync; you may see it in ops information.',
      notes: 'Don’t set extremely small values “for safety” without reason — you’ll hurt friend sites.',
    },
    rateMax: {
      what: 'Maximum requests accepted in a period of time.',
      chain:
        '1) Together with the “time window” below, decides how tight the limit is.\n2) Over limit → reject the other side for a while.\n3) Trusted sites can also be multiplied higher.',
      frontend: 'When over limit, the other side can’t connect.',
      notes: 'Too small hurts normal traffic.',
    },
    rateWindow: {
      what: 'How long a period the count above is measured over.',
      chain:
        '1) When the window ends, the count restarts.\n2) Short window + small max = easier to hit the limit.',
      frontend: 'Works together with the request max.',
      notes: 'Fill within the range the page shows.',
    },
    rateTrusted: {
      what: 'How many times higher the limit is for more trusted sites.',
      chain:
        '1) Friend sites are less likely to be blocked.\n2) Very large multiplier is almost unlimited.\n3) Depends on trust levels you set under known instances.',
      frontend: 'Friendlier sites feel smoother.',
      notes: 'Only give high multipliers to sites you truly trust.',
    },
  },

  updater: {
    channel: {
      what: 'Which release line upgrades follow: stable, preview, or development.',
      chain:
        '1) After you pick one, the channel is remembered.\n2) “Check for updates” looks for new versions on that line.\n3) You confirm the upgrade → may enter maintenance → backup → switch version → open again.\n4) The visitor home page does not change just because you “picked a channel” — only after the upgrade finishes.',
      frontend: 'Version and notes shown in About / updater areas of settings.',
      notes: 'For real use, prefer the stable (official) channel; preview/dev can be unstable.',
    },
    maintenance: {
      what: 'How to recover when an upgrade problem leaves the site in maintenance.',
      chain:
        '1) A failed upgrade stops on the maintenance page.\n2) Prefer “restore from the pre-upgrade backup.”\n3) “Force exit maintenance” only closes the maintenance page — data may still be half-updated.\n4) After rescue, check whether the site is healthy again.',
      frontend: 'Maintenance notice page versus the normal site.',
      notes: 'If rescue is available, rescue first — don’t force-exit first.',
    },
    rescue: {
      what: 'Use the pre-upgrade backup to restore data and state.',
      chain:
        '1) Choose rescue → restore backup → services come up → leave maintenance.\n2) The site returns to the moment of that backup.\n3) Relatively the safest path.',
      frontend: 'After maintenance ends, site content/settings match the backup point.',
      notes: 'Before rescuing, confirm this is the point in time you want to return to.',
    },
    forceExit: {
      what: 'Leave maintenance without restoring a backup.',
      chain:
        '1) Maintenance page closes.\n2) Half-updated data is not auto-fixed.\n3) You may open the site but with broken features.',
      frontend: 'Site opens, but content may look wrong.',
      notes: 'Only when you accept the current state; otherwise rescue first.',
    },
    infra: {
      what: 'Versions of helper components that perform upgrades.',
      chain:
        '1) Separate line from “main website program upgrade.”\n2) Upgrading them may also briefly enter maintenance.\n3) When a new version is available, this area prompts you.',
      frontend: 'Ops hints; ordinary visitors usually don’t notice.',
      notes: 'Sometimes these components need updating before the main site update.',
    },
    target: {
      what: 'Install a particular old or specified version.',
      chain:
        '1) Similar to a normal upgrade, but you choose the version.\n2) Features may shrink or behave differently.\n3) Wrong choice may leave the site unable to start.',
      frontend: 'Site-wide features may roll back.',
      notes: 'Not for daily use; if something breaks, use rescue rather than picking random versions.',
    },
    snapshot: {
      what: 'List of automatic backups taken before upgrades.',
      chain:
        '1) Auto backup before upgrade.\n2) On problems you can roll back or rescue from here.\n3) How many you can keep depends on disk space; enable Backup limit to keep only the newest few.',
      frontend: 'After rollback, content and settings return to the backup point.',
      notes: 'Before important upgrades, confirm a fresh snapshot exists.',
    },
    snapshotLimit: {
      what: 'Auto-prune older backups so only the newest few remain.',
      chain:
        '1) When on, pick “Latest N” on the right; extras beyond that are removed after a successful update, after a failed update that already created a backup, or when you change this setting.\n2) Kept pins and backups in use by recovery are never auto-deleted; age does not exempt a backup from the limit.\n3) Off = no count-based auto-prune (manual delete still works).',
      frontend: 'Top of Backups & rollback: label left, keep-count + switch right.',
      notes: 'Default on, keep 3. Use 1–2 when disk is tight; raise for a longer rollback window.',
    },
    checkInterval: {
      what: 'How often the background checks for a new version.',
      chain:
        '1) Changing the interval in the status card is remembered immediately.\n2) On schedule it checks the current update channel.\n3) Found updates are announced; auto-install decides whether they install alone.\n4) Interval off = no automatic checks; manual check still works.',
      frontend: '“Check frequency” dropdown on the About/Updater status card. No full-page save needed.',
      notes: '12h or 24h is fine in production; very short intervals only add noise.',
    },
    autoInstall: {
      what: 'When a suitable new version appears, whether to upgrade without someone watching.',
      chain:
        '1) On: a suitable version may auto-start upgrade → maintenance → switch version.\n2) Snapshots are usually taken first; failures can use maintenance rescue.\n3) Off: only notify; you click install.',
      frontend: '“Auto-install” switch on the status card. On may suddenly show the maintenance page.',
      notes: 'Strongly recommended off for public production; preview/private nets only if you accept risk.',
    },
    transport: {
      what: 'Whether update checks/downloads go through this site’s backend proxy or direct from the browser.',
      chain:
        '1) Default “backend”: the server talks to update sources; visitor browsers do not.\n2) “Direct”: the browser uses an ops token to call update APIs — special deployments only.\n3) Unrelated to check frequency / auto-install — only how you connect.',
      frontend: 'Transport segmented control under Advanced & diagnostics.',
      notes: 'If unsure, keep backend; direct needs deployment knowledge and a token.',
    },
    token: {
      what: 'Ops passphrase (UPDATE_TOKEN) for direct-mode update APIs.',
      chain:
        '1) Only required in direct mode.\n2) Wrong or empty values fail check/upgrade.\n3) Session/local input only — never post publicly.',
      frontend: 'UPDATE_TOKEN password field under direct transport.',
      notes: 'Not your login password; fill from deployment docs.',
    },
    advanced: {
      what: 'How the updater connects, plus ops diagnostics.',
      chain:
        '1) Default path uses this site’s backend to proxy update APIs.\n2) “Direct” mode may need UPDATE_TOKEN — only if you understand the deployment.\n3) Shows updater version, channel, in-flight jobs, and image digests.\n4) Check frequency / auto-install live in the status card above, not this panel.',
      frontend: '“Advanced & diagnostics” collapsible panel.',
      notes: 'Don’t share the token; if connection fails, confirm whether direct mode is allowed.',
    },
  },

  about: {
    section: {
      what: 'View current version and project info; this page usually also embeds the system updater.',
      chain:
        '1) Upper area is read-only version and project info.\n2) Lower updater: channel, check frequency, auto-install, maintenance rescue, snapshots, and advanced diagnostics.\n3) When reporting errors, send the version number to maintainers.',
      frontend: 'About text, version number, and the same-page updater status card and groups.',
      notes: 'Reading version does not change business settings; starting an upgrade enters maintenance.',
    },
  },

  tapp: {
    detail: {
      what: 'Detail page for one Tapp: identity, settings, start/stop, and uninstall.',
      chain:
        '1) Top card is identity plus actions (start/stop, export, uninstall).\n2) App settings save to this app’s per-user config (immediate or on blur).\n3) Permissions are a read-only list of capabilities granted at install time.',
      frontend: 'Open an app from the Tapp list, or go to /tapp/detail/:id.',
      notes:
        'Guests usually can only view. Changing settings / start-stop / uninstall needs the right role (admin, or your own temporary install).\nUninstall asks whether to keep local data.',
    },
    overview: {
      what: 'App profile and common actions: id, version, author, install time, plus start, export, uninstall.',
      chain:
        '1) Start opens the run page; stop if already running.\n2) Export downloads a package to your machine.\n3) Uninstall removes the install (optionally keeping data).',
      frontend: 'Top info card and action row on the detail page.',
      notes:
        'App id is stable; renaming the display name does not change it.\nExport is for backup/migration; you must reinstall after uninstall.',
    },
    appSettings: {
      what: 'Behavior options declared by the app manifest, plus install visibility for admins when applicable.',
      chain:
        '1) Toggles save immediately; text/number fields save after idle or blur.\n2) Values are stored per app id and only affect this app.\n3) Public-install global settings may be read-only unless you are the installer/admin.',
      frontend: '“App settings” group on the detail page.',
      notes:
        'If the manifest has no settings, you may only see visibility or an empty state.\nChanges usually apply on the next run without a site-wide “Save config”.',
    },
    appVisibility: {
      what: 'Who can see this site-level public install in the app list: everyone, or admins only.',
      chain:
        '1) Only admins, and only for public installs (not pure personal temp installs).\n2) “Admins only” hides it from regular users’ lists.\n3) Saves immediately and refreshes list caches.',
      frontend: '“App visibility” segmented control under App settings.',
      notes: 'Does not remove a user’s own temporary install; only site-level list visibility.',
    },
    permissions: {
      what: 'Capabilities declared and granted at install, grouped by risk level.',
      chain:
        '1) Read-only here — you cannot add/remove grants on this page.\n2) Runtime APIs enforce these permissions.\n3) Higher risk groups appear first (privileged → elevated → basic).',
      frontend: '“Permissions” group and level subgroups.',
      notes:
        'Changing grants usually means reinstall with a new manifest, or an app update flow.\nNo permissions means the app needs no extra capabilities.',
    },
    permPrivileged: {
      what: 'Privileged permissions: site-level or sensitive write capabilities; highest risk.',
      chain:
        '1) Examples: home widgets, writing platform data, managing Tapp list, federation trust.\n2) Only for trusted apps.\n3) Unauthorized calls are rejected at runtime.',
      frontend: '“Privileged” subgroup under Permissions.',
      notes: 'If you do not recognize a privileged grant, verify the app source before use.',
    },
    permElevated: {
      what: 'Elevated permissions: AI, network, schedulers, and other cost or outbound capabilities.',
      chain:
        '1) Examples: AI generate/chat, network fetch, speech, shortcuts, event publish.\n2) May consume quotas or call external services.\n3) Still bounded by site-wide AI/network configuration.',
      frontend: '“Elevated” subgroup under Permissions.',
      notes: 'Quota exhaustion or proxy failures can break these features without the app itself being broken.',
    },
    permBasic: {
      what: 'Basic permissions: routine read/write, notifications, theme, and similar lower-risk capabilities.',
      chain:
        '1) Examples: read platform data, read reports, storage, notifications, media control.\n2) Usually does not change core site configuration.\n3) Still requires an explicit grant to call.',
      frontend: '“Basic” subgroup under Permissions.',
      notes: 'Basic is not unlimited — undeclared capabilities remain blocked.',
    },
  },
}
