/**
 * Aro — 社交中心 Tapp
 *
 * 统一管理消息 (Channel/Room)、时间线、环网、个人资料
 * 展示 Tapp.federation.* SDK 的完整用法
 */

import type { TappManifest } from '../../types'
import type { ExampleTapp, TappCodeStructure } from './types'

// ==================== Page HTML ====================
const PAGE_HTML = `\
<!-- 背景层 -->
<div id="tapp-background">
  <div class="page-bg-base"></div>
  <div class="page-bg-glow page-bg-glow-1"></div>
  <div class="page-bg-glow page-bg-glow-2"></div>
</div>

<!-- 内容层 -->
<div id="tapp-content">
  <!-- 顶部导航栏 -->
  <nav id="aro-nav" class="aro-nav">
    <button class="aro-nav-item aro-nav-active" data-view="feed" id="nav-feed">
      <div id="nav-feed-avatar" class="nav-feed-avatar">?</div>
      <span id="nav-feed-label" class="nav-feed-name"></span>
    </button>
    <button class="aro-nav-item" data-view="messages" id="nav-messages">
      <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 2L11 13"/><path d="M22 2l-7 20-4-9-9-4 20-7z"/></svg>
      <span id="nav-messages-label">信使</span>
    </button>
    <button class="aro-nav-item" data-view="rings" id="nav-rings">
      <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M0 15L24 9"/></svg>
      <span id="nav-rings-label">环网</span>
    </button>
  </nav>

  <!-- ====== 信使视图 ====== -->
  <div id="view-messages" class="aro-view">
    <div class="messenger-app">
      <!-- 左侧：会话列表 -->
      <aside id="sidebar" class="sidebar">
        <div class="sidebar-header">
          <h2 class="sidebar-title">信使</h2>
          <button id="create-btn" class="create-btn" title="新建">+</button>
        </div>
        <div id="conv-list" class="conv-list"></div>
      </aside>

      <!-- 中间：聊天区 -->
      <main id="chat-main" class="chat-main">
        <div id="empty-state" class="empty-state">
          <div class="empty-icon"><svg viewBox="0 0 24 24" width="32" height="32" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="16" rx="2"/><path d="M22 4L12 13 2 4"/></svg></div>
          <p class="empty-text">选择一个会话开始聊天</p>
        </div>
        <div id="chat-container" class="chat-container" style="display:none">
          <div id="chat-header" class="chat-header">
            <button id="back-btn" class="back-btn">←</button>
            <div id="chat-hdr-avatar" class="chat-hdr-avatar"></div>
            <div class="chat-header-info">
              <div id="chat-name" class="chat-name"></div>
              <div id="chat-meta" class="chat-meta"></div>
            </div>
            <div id="chat-actions" class="chat-actions"></div>
          </div>
          <div id="pinned-bar" class="pinned-bar" style="display:none"></div>
          <div id="messages" class="messages-area"></div>
          <div class="input-float-wrap">
            <div id="quote-preview" class="quote-preview" style="display:none"></div>
            <div id="attach-preview" class="attach-preview" style="display:none"></div>
            <div id="input-bar" class="input-bar">
              <button id="attach-btn" class="attach-btn" title="附件">
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
              </button>
              <textarea id="msg-input" class="msg-input" rows="1" placeholder="输入消息..."></textarea>
              <button id="send-btn" class="send-btn" disabled aria-label="Send">
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M12 20V4M5 11l7-7 7 7"/>
                </svg>
              </button>
            </div>
          </div>
          <input id="attach-file-input" type="file" style="display:none" />
          <input id="attach-image-input" type="file" accept="image/*" style="display:none" />
        </div>
      </main>

      <!-- 右侧：成员面板 -->
      <aside id="member-panel" class="member-panel" style="display:none">
        <div class="member-header">
          <button id="member-back-btn" class="member-back-btn">←</button>
          <h3 id="member-title" class="member-title">成员</h3>
          <div id="invite-wrap" class="invite-wrap" style="display:none">
            <button id="invite-toggle" class="invite-toggle" title="邀请成员">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 5v14M5 12h14"/></svg>
            </button>
          </div>
        </div>
        <div id="member-list" class="member-list"></div>
      </aside>
    </div>
  </div>

  <!-- ====== 动态视图 (Feed, X-style sidebar + content) ====== -->
  <div id="view-feed" class="aro-view aro-view-active">
    <div class="feed-layout">
      <!-- Left sidebar -->
      <aside class="feed-sidebar" id="feed-sidebar">
        <nav class="feed-sidebar-nav">
          <button class="feed-nav-item feed-nav-active" data-sub="timeline">
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2V9z"/><path d="M9 22V12h6v10"/></svg>
            <span id="feed-nav-timeline">动态</span>
          </button>
          <button class="feed-nav-item" data-sub="following">
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2"><path d="M16 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="8.5" cy="7" r="4"/><path d="M20 8v6M23 11h-6"/></svg>
            <span id="feed-nav-following">关注</span>
          </button>
          <button class="feed-nav-item" data-sub="followers">
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 00-3-3.87M16 3.13a4 4 0 010 7.75"/></svg>
            <span id="feed-nav-followers">粉丝</span>
          </button>
          <button class="feed-nav-item" data-sub="published">
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6M16 13H8M16 17H8M10 9H8"/></svg>
            <span id="feed-nav-published">已发布</span>
          </button>
        </nav>
        <div class="feed-sidebar-footer">
          <div class="feed-profile-card" data-fed-profile>
            <div class="feed-profile-summary" data-fed-toggle role="button" tabindex="0" aria-expanded="false">
              <div id="feed-avatar" class="feed-avatar" data-feed-avatar>?</div>
              <div class="feed-profile-info">
                <div id="feed-display-name" class="feed-display-name" data-feed-display-name></div>
                <div id="feed-handle" class="feed-handle" data-fed-handle-summary></div>
              </div>
              <button class="feed-profile-copy" data-copy-fed="handle" title="复制">
                <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
              </button>
              <button class="feed-profile-toggle" data-fed-toggle-button title="展开">
                <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>
              </button>
            </div>
            <div class="feed-profile-details" data-fed-details>
              <button class="feed-identity-actor" data-copy-fed="actor" type="button" data-fed-actor></button>
            </div>
            <div class="feed-profile-tablet-popover" data-fed-tablet-popover>
              <div class="feed-profile-tablet-main">
                <div class="feed-profile-info">
                  <div class="feed-display-name" data-feed-display-name></div>
                  <div class="feed-handle" data-fed-handle-summary></div>
                </div>
                <button class="feed-profile-copy" data-copy-fed="handle" title="复制">
                  <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
                </button>
                <button class="feed-profile-toggle" data-fed-toggle-button title="展开">
                  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>
                </button>
              </div>
              <div class="feed-profile-details" data-fed-details>
                <button class="feed-identity-actor" data-copy-fed="actor" type="button" data-fed-actor></button>
              </div>
            </div>
          </div>
          <div class="feed-sidebar-stats">
            <div class="feed-sidebar-stat">
              <span class="feed-stat-num" id="feed-count-following">0</span>
              <span class="feed-stat-lbl" id="feed-lbl-following">关注</span>
            </div>
            <div class="feed-sidebar-stat">
              <span class="feed-stat-num" id="feed-count-followers">0</span>
              <span class="feed-stat-lbl" id="feed-lbl-followers">粉丝</span>
            </div>
            <div class="feed-sidebar-stat">
              <span class="feed-stat-num" id="feed-count-published">0</span>
              <span class="feed-stat-lbl" id="feed-lbl-published">已发布</span>
            </div>
          </div>
        </div>
      </aside>
      <!-- Main content -->
      <main class="feed-main">
        <div class="feed-main-header">
          <div class="feed-header-leading">
            <div class="feed-main-heading">
              <div id="feed-section-title" class="feed-section-title">动态</div>
              <div id="feed-section-meta" class="feed-section-meta"></div>
            </div>
          </div>
          <div class="feed-header-actions">
            <button id="refresh-feed-btn" class="feed-refresh-btn" title="刷新">
              <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
            </button>
            <button id="feed-compose-btn" class="feed-compose-btn" type="button" title="发帖" style="display:none">
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>
              <span id="feed-compose-btn-label">发帖</span>
            </button>
          </div>
        </div>
        <!-- Freeform note composer (owner only) -->
        <div id="feed-composer" class="feed-composer" style="display:none">
          <textarea id="feed-compose-text" class="feed-compose-text" rows="3" placeholder="分享点什么…"></textarea>
          <div id="feed-compose-previews" class="feed-compose-previews"></div>
          <div class="feed-compose-actions">
            <div class="feed-compose-attach">
              <button type="button" id="feed-compose-image-btn" class="feed-compose-tool" title="图片">
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>
                <span id="feed-compose-image-label">图片</span>
              </button>
              <button type="button" id="feed-compose-video-btn" class="feed-compose-tool" title="视频">
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="4" width="20" height="16" rx="2"/><path d="M10 9l5 3-5 3V9z"/></svg>
                <span id="feed-compose-video-label">视频</span>
              </button>
              <input id="feed-compose-image-input" type="file" accept="image/jpeg,image/png,image/gif,image/webp" multiple style="display:none" />
              <input id="feed-compose-video-input" type="file" accept="video/mp4,video/webm,video/quicktime" multiple style="display:none" />
            </div>
            <div class="feed-compose-submit">
              <button type="button" id="feed-compose-cancel" class="feed-compose-cancel">取消</button>
              <button type="button" id="feed-compose-publish" class="feed-compose-publish">发布</button>
            </div>
          </div>
        </div>
        <!-- Mobile tabs (visible only on small screens) -->
        <div class="feed-mobile-tabs" id="feed-mobile-tabs">
          <button id="feed-compose-mobile-btn" class="feed-mobile-compose" type="button" title="发帖" style="display:none">
            <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>
          </button>
          <button class="feed-mobile-tab feed-mobile-tab-active" data-sub="timeline" id="feed-tab-timeline">动态</button>
          <button class="feed-mobile-tab" data-sub="following" id="feed-tab-following">关注</button>
          <button class="feed-mobile-tab" data-sub="followers" id="feed-tab-followers">粉丝</button>
          <button class="feed-mobile-tab" data-sub="published" id="feed-tab-published">已发布</button>
          <button id="refresh-feed-mobile-btn" class="feed-mobile-refresh" title="刷新">
            <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
          </button>
        </div>
        <div class="feed-profile-card feed-profile-mobile" data-fed-profile>
          <div class="feed-profile-summary" data-fed-toggle role="button" tabindex="0" aria-expanded="false">
            <div class="feed-avatar" data-feed-avatar>?</div>
            <div class="feed-profile-info">
              <div class="feed-display-name" data-feed-display-name></div>
              <div class="feed-handle" data-fed-handle-summary></div>
            </div>
            <button class="feed-profile-copy" data-copy-fed="handle" title="复制">
              <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
            </button>
            <button class="feed-profile-toggle" data-fed-toggle-button title="展开">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>
            </button>
          </div>
          <div class="feed-profile-details" data-fed-details>
            <button class="feed-identity-actor" data-copy-fed="actor" type="button" data-fed-actor></button>
          </div>
        </div>
        <div class="feed-mobile-stats">
          <div class="feed-mobile-stat">
            <span class="feed-stat-num" id="feed-mobile-count-following">0</span>
            <span class="feed-stat-lbl" id="feed-mobile-lbl-following">关注</span>
          </div>
          <div class="feed-mobile-stat">
            <span class="feed-stat-num" id="feed-mobile-count-followers">0</span>
            <span class="feed-stat-lbl" id="feed-mobile-lbl-followers">粉丝</span>
          </div>
          <div class="feed-mobile-stat">
            <span class="feed-stat-num" id="feed-mobile-count-published">0</span>
            <span class="feed-stat-lbl" id="feed-mobile-lbl-published">已发布</span>
          </div>
        </div>
        <div id="feed-follow-bar" class="feed-follow-bar" style="display:none">
          <input id="feed-follow-input" class="feed-follow-input" type="text" placeholder="Actor URL 或 @user@domain" />
          <button id="feed-follow-btn" class="feed-follow-btn">关注</button>
        </div>
        <div id="feed-content" class="feed-content"></div>
        <div id="feed-empty" class="feed-empty" style="display:none">
          <div class="aro-empty-mark feed-empty-mark"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5a14 14 0 0114 14"/><path d="M5 11a8 8 0 018 8"/><circle cx="6" cy="18" r="1.6"/></svg></div>
          <div id="feed-empty-title" class="feed-empty-title">动态</div>
          <span id="feed-empty-text">暂无内容</span>
        </div>
      </main>
    </div>
  </div>

  <!-- ====== 环网视图 ====== -->
  <div id="view-rings" class="aro-view">
    <div class="aro-panel-layout">
      <!-- 左侧：环网列表 -->
      <aside id="ring-sidebar" class="sidebar">
        <div class="sidebar-header">
          <h2 id="ring-sidebar-title" class="sidebar-title">环网</h2>
          <button id="ring-create-open-btn" class="create-btn" title="新建">+</button>
        </div>
        <div id="ring-list" class="conv-list"></div>
      </aside>
      <!-- 右侧：环网详情 -->
      <main class="panel-main">
        <div id="ring-empty-state" class="empty-state">
          <div class="empty-icon"><svg viewBox="0 0 24 24" width="32" height="32" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M0 15L24 9"/></svg></div>
          <p class="empty-text" id="ring-select-hint">选择一个环网查看详情</p>
        </div>
        <div id="ring-detail" class="panel-detail" style="display:none">
          <div class="panel-detail-header">
            <button id="ring-back-btn" class="back-btn">←</button>
            <div id="ring-detail-icon" class="ring-hdr-icon"><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M0 15L24 9"/></svg></div>
            <div class="chat-header-info">
              <div id="ring-detail-name" class="chat-name"></div>
              <div id="ring-detail-meta" class="chat-meta"></div>
            </div>
            <div class="chat-actions">
              <button id="ring-sync-btn" class="action-btn ring-action-sync" title="同步">
                <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"/></svg>
                <span id="ring-sync-label">同步</span>
              </button>
              <div class="manage-wrap">
                <button id="ring-manage-btn" class="manage-btn">⋯</button>
                <div id="ring-manage-dropdown" class="manage-dropdown">
                  <button id="ring-leave-btn" class="manage-item manage-item-danger">
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4M16 17l5-5-5-5M21 12H9"/></svg>
                    <span id="ring-leave-label">退出环网</span>
                  </button>
                </div>
              </div>
            </div>
          </div>
          <!-- 同步状态 -->
          <div id="ring-sync-status" class="ring-sync-bar" style="display:none"></div>
          <!-- 添加节点 -->
          <div class="invite-bar" id="ring-peer-bar">
            <input id="ring-peer-input" class="invite-input" type="text" placeholder="Actor URL 或 @user@domain" />
            <button id="ring-add-peer-btn" class="invite-btn">添加</button>
          </div>
          <!-- 节点列表 -->
          <div id="ring-peers-list" class="member-list"></div>
          <div id="ring-peers-empty" class="conv-empty" style="display:none">
            <span>暂无节点</span>
          </div>
        </div>
      </main>
    </div>
  </div>

  <!-- 创建环网对话框 -->
  <div id="ring-create-dialog" class="create-overlay" style="display:none">
    <div class="create-dialog">
      <div class="create-dialog-header">
        <h3 id="ring-create-title" class="create-dialog-title">创建环网</h3>
        <button id="ring-create-close" class="create-dialog-close">✕</button>
      </div>
      <div class="create-form">
        <input id="ring-name-input" class="create-input" type="text" placeholder="环网名称" />
        <select id="ring-type-select" class="create-input" style="height:40px;cursor:pointer">
          <option id="ring-type-opt-brew" value="brew-recommend">Brew 推荐</option>
          <option id="ring-type-opt-tapp" value="tapp-store">Tapp 商店</option>
          <option id="ring-type-opt-library" value="library-exchange">资料交换</option>
          <option id="ring-type-opt-instance" value="instance-directory">实例目录</option>
        </select>
        <button id="create-ring-btn" class="create-submit">创建</button>
      </div>
    </div>
  </div>

  <!-- 创建对话框 -->
  <div id="create-dialog" class="create-overlay" style="display:none">
    <div class="create-dialog">
      <div class="create-dialog-header">
        <h3 id="create-dialog-title" class="create-dialog-title">新建</h3>
        <button id="create-dialog-close" class="create-dialog-close">✕</button>
      </div>
      <div class="create-dialog-tabs">
        <button id="create-tab-channel" class="create-tab create-tab-active" data-tab="channel">私信</button>
        <button id="create-tab-room" class="create-tab" data-tab="room">群聊</button>
      </div>
      <div id="create-form-channel" class="create-form">
        <input id="create-channel-input" class="create-input" type="text" placeholder="Actor URL 或 @user@domain" />
        <button id="create-channel-btn" class="create-submit">创建通道</button>
      </div>
      <div id="create-form-room" class="create-form" style="display:none">
        <input id="create-room-input" class="create-input" type="text" placeholder="房间名称" />
        <button id="create-room-btn" class="create-submit">创建房间</button>
      </div>
    </div>
  </div>

  <!-- 编辑房间对话框 -->
  <div id="edit-room-dialog" class="create-overlay" style="display:none">
    <div class="create-dialog">
      <div class="create-dialog-header">
        <h3 id="edit-room-title" class="create-dialog-title">编辑房间</h3>
        <button id="edit-room-close" class="create-dialog-close">✕</button>
      </div>
      <div class="create-form">
        <label class="edit-label" id="edit-name-label">房间名称</label>
        <input id="edit-room-name" class="create-input" type="text" />
        <label class="edit-label" id="edit-desc-label">房间描述</label>
        <input id="edit-room-desc" class="create-input" type="text" />
        <button id="edit-room-save" class="create-submit">保存</button>
      </div>
    </div>
  </div>
</div>
`

const STYLES = `\
/* ===== Dark Mode Variable Bridge ===== */
:root{--text-primary:#1a1a1a;--text-secondary:#999;--bg-primary:#fff}
.dark{--text-primary:rgba(255,255,255,.92);--text-secondary:rgba(255,255,255,.5);--bg-primary:#0a0a0a;color-scheme:dark}
body.dark{background:#0a0a0a;color:rgba(255,255,255,.92)}

/* ===== Tapp Content Override ===== */
#tapp-content{display:flex!important;flex-direction:column!important;overflow:hidden!important}

/* ===== Aro Nav ===== */
.aro-nav{display:flex;align-items:center;gap:6px;padding:8px 10px;border-bottom:1px solid rgba(128,128,128,.08);flex-shrink:0;background:rgba(255,255,255,.72);backdrop-filter:blur(16px)}
.aro-nav-item{height:34px;display:flex;align-items:center;gap:7px;padding:0 12px;border:none;background:none;border-radius:12px;font-size:12px;font-weight:600;color:var(--text-secondary,#999);cursor:pointer;transition:background .15s,color .15s,box-shadow .15s;white-space:nowrap}
.aro-nav-item:hover{background:rgba(128,128,128,.08);color:var(--text-primary,#333)}
.aro-nav-active{background:rgba(var(--tapp-primary-rgb,100,100,255),.14)!important;color:var(--tapp-primary,#6366f1)!important;box-shadow:inset 0 0 0 1px rgba(var(--tapp-primary-rgb,100,100,255),.08)}
.aro-nav-item svg{flex-shrink:0}
/* Nav Feed Avatar */
.nav-feed-avatar{width:22px;height:22px;border-radius:50%;background:rgba(var(--tapp-primary-rgb,128,128,128),.14);color:var(--tapp-primary,#888);display:flex;align-items:center;justify-content:center;font-size:10px;font-weight:700;flex-shrink:0;overflow:hidden}
.nav-feed-avatar img{width:100%;height:100%;object-fit:cover}
.nav-feed-name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:80px}

/* ===== Aro Views ===== */
.aro-view{display:none;flex:1;min-height:0;overflow:hidden}
.aro-view-active{display:flex;flex-direction:column}

/* ===== Panel Layout (sidebar+content, for rings) ===== */
.aro-panel-layout{display:flex;flex:1;min-height:0;overflow:hidden}
.panel-main{flex:1;min-width:0;display:flex;flex-direction:column;position:relative;overflow:hidden}
.panel-detail{flex:1;min-height:0;display:flex;flex-direction:column}
.panel-detail-header{display:flex;align-items:center;gap:10px;padding:10px 14px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0}
.panel-detail-body{flex:1;overflow-y:auto;padding:16px}

/* ===== Ring header icon ===== */
.ring-hdr-icon{width:32px;height:32px;border-radius:8px;flex-shrink:0;display:flex;align-items:center;justify-content:center;font-size:16px;background:rgba(var(--tapp-primary-rgb,128,128,128),.08)}
.ring-action-sync{display:flex;align-items:center;gap:4px;background:rgba(var(--tapp-primary-rgb,100,100,255),.08);color:var(--tapp-primary,#6366f1);border:none;border-radius:10px;padding:4px 10px;font-size:11px;font-weight:500;cursor:pointer;transition:background .15s}
.ring-action-sync:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.16)}
.ring-sync-bar{font-size:11px;padding:8px 14px;border-bottom:1px solid rgba(128,128,128,.06);display:flex;align-items:center;gap:6px}
.ring-sync-bar.ring-sync-ok{color:#22c55e;background:rgba(34,197,94,.04)}
.ring-sync-bar.ring-sync-err{color:#ef4444;background:rgba(239,68,68,.04)}

/* ===== Feed Layout (X-style sidebar + content) ===== */
.feed-layout{display:flex;flex:1;min-height:0;overflow:hidden}
/* Feed Sidebar */
.feed-sidebar{width:232px;flex-shrink:0;border-right:1px solid rgba(128,128,128,.08);display:flex;flex-direction:column;padding:14px 12px;overflow:hidden}
.feed-avatar{width:36px;height:36px;border-radius:50%;background:rgba(var(--tapp-primary-rgb,128,128,128),.1);color:var(--tapp-primary,#888);display:flex;align-items:center;justify-content:center;font-size:14px;font-weight:600;flex-shrink:0;overflow:hidden}
.feed-avatar img{width:100%;height:100%;object-fit:cover}
.feed-display-name{font-size:13px;font-weight:700;color:var(--text-primary,#0f1419);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:100%}
.feed-handle{font-size:11px;color:var(--text-secondary,#536471);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:100%}
/* Sidebar Nav */
.feed-sidebar-nav{display:flex;flex-direction:column;gap:4px;padding:2px 0 8px;flex:1;overflow-y:auto;min-height:0}
.feed-nav-item{display:flex;align-items:center;gap:13px;padding:11px 13px;border:none;background:none;border-radius:14px;font-size:14px;font-weight:600;color:var(--text-primary,#0f1419);cursor:pointer;transition:background .15s,color .15s;white-space:nowrap;text-align:left;width:100%}
.feed-nav-item:hover{background:rgba(128,128,128,.08)}
.feed-nav-active{background:rgba(var(--tapp-primary-rgb,100,100,255),.1)!important;color:var(--tapp-primary,#6366f1);font-weight:700!important}
.feed-nav-active svg{stroke-width:2.5}
.feed-nav-item svg{flex-shrink:0}
/* Sidebar Footer (profile + stats) */
.feed-sidebar-footer{flex-shrink:0;border-top:1px solid rgba(128,128,128,.06);padding-top:12px;display:flex;flex-direction:column;gap:10px}
.feed-sidebar-stats{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px}
.feed-sidebar-stat{min-width:0;display:flex;flex-direction:column;align-items:flex-start;gap:2px;padding:7px 8px;border-radius:9px;background:rgba(128,128,128,.045);font-size:12px;color:var(--text-secondary,#536471)}
.feed-stat-num{font-weight:700;color:var(--text-primary,#0f1419)}
.feed-stat-lbl{max-width:100%;font-size:10px;font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.feed-profile-card{display:flex;flex-direction:column;gap:0;padding:8px;border:1px solid rgba(128,128,128,.12);border-radius:12px;background:rgba(128,128,128,.035);min-width:0}
.feed-profile-summary{min-width:0;display:flex;align-items:center;gap:9px;border-radius:10px;cursor:pointer;outline:none}
.feed-profile-summary:focus-visible{box-shadow:0 0 0 2px rgba(var(--tapp-primary-rgb,100,100,255),.35)}
.feed-profile-info{min-width:0;flex:1;display:flex;flex-direction:column;gap:1px}
.feed-profile-copy,.feed-profile-toggle{width:28px;height:28px;border:none;border-radius:9px;background:rgba(var(--tapp-primary-rgb,100,100,255),.08);color:var(--tapp-primary,#6366f1);display:flex;align-items:center;justify-content:center;cursor:pointer;flex-shrink:0;transition:background .15s,transform .15s}
.feed-profile-copy:hover,.feed-profile-toggle:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.14)}
.feed-profile-toggle{background:rgba(128,128,128,.06);color:var(--text-secondary,#536471)}
.feed-profile-card.feed-profile-expanded .feed-profile-toggle svg{transform:rotate(180deg)}
.feed-profile-details{max-height:0;overflow:hidden;opacity:0;transition:max-height .18s ease,opacity .15s ease,padding-top .18s ease}
.feed-profile-card.feed-profile-expanded .feed-profile-details{max-height:56px;opacity:1;padding-top:8px}
.feed-identity-actor{width:100%;padding:9px 10px;border:none;border-radius:9px;background:rgba(128,128,128,.06);text-align:left;cursor:pointer;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0;font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;font-size:11px;line-height:1.35;color:var(--text-primary,#0f1419)}
.feed-profile-card.feed-identity-actor-missing .feed-profile-details{display:none}
.feed-identity-actor:disabled{cursor:default}
.feed-profile-tablet-popover{display:none}
.feed-profile-tablet-main{display:flex;align-items:center;gap:9px;min-width:0}
.feed-profile-mobile{display:none;margin:10px 16px 0;flex-shrink:0}
.feed-mobile-stats{display:none;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px;margin:8px 16px 0;flex-shrink:0}
.feed-mobile-stat{min-width:0;display:flex;align-items:center;justify-content:space-between;gap:6px;padding:8px 10px;border-radius:10px;background:rgba(128,128,128,.04);color:var(--text-secondary,#536471);font-size:11px}
/* Feed Main */
.feed-main{flex:1;min-width:0;max-width:760px;display:flex;flex-direction:column;overflow-y:auto;position:relative;border-right:1px solid rgba(128,128,128,.06)}
.dark .feed-main{border-right-color:rgba(255,255,255,.05)}
/* Feed header */
.feed-main-header{min-height:56px;display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 16px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0;background:rgba(255,255,255,.45);backdrop-filter:blur(12px)}
.feed-header-leading{display:flex;align-items:center;gap:10px;min-width:0;flex:1}
.feed-compose-btn{display:inline-flex;align-items:center;gap:6px;padding:7px 12px;border-radius:999px;border:none;background:var(--tapp-primary,#6366f1);color:#fff;font-size:13px;font-weight:600;cursor:pointer;flex-shrink:0;transition:opacity .15s,transform .12s}
.feed-compose-btn:hover{opacity:.92}
.feed-compose-btn:active{transform:scale(.97)}
.feed-composer{padding:12px 16px;border-bottom:1px solid rgba(128,128,128,.08);background:rgba(255,255,255,.35);flex-shrink:0}
.dark .feed-composer{background:rgba(0,0,0,.25);border-color:rgba(255,255,255,.06)}
.feed-compose-text{width:100%;min-height:72px;resize:vertical;border:1px solid rgba(128,128,128,.15);border-radius:12px;padding:10px 12px;font-size:14px;line-height:1.5;background:rgba(255,255,255,.7);color:var(--text-primary,#1a1a1a);outline:none;font-family:inherit}
.dark .feed-compose-text{background:rgba(20,20,20,.7);border-color:rgba(255,255,255,.1);color:rgba(255,255,255,.92)}
.feed-compose-text:focus{border-color:var(--tapp-primary,#6366f1);box-shadow:0 0 0 3px rgba(99,102,241,.15)}
.feed-compose-previews{display:flex;flex-wrap:wrap;gap:8px;margin-top:10px}
.feed-compose-preview{position:relative;width:88px;height:88px;border-radius:10px;overflow:hidden;background:rgba(128,128,128,.08);border:1px solid rgba(128,128,128,.12)}
.feed-compose-preview img,.feed-compose-preview video{width:100%;height:100%;object-fit:cover;display:block}
.feed-compose-preview-remove{position:absolute;top:4px;right:4px;width:22px;height:22px;border:none;border-radius:50%;background:rgba(0,0,0,.55);color:#fff;cursor:pointer;font-size:14px;line-height:1;display:flex;align-items:center;justify-content:center}
.feed-compose-actions{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-top:10px;flex-wrap:wrap}
.feed-compose-attach{display:flex;gap:6px}
.feed-compose-tool{display:inline-flex;align-items:center;gap:5px;padding:6px 10px;border-radius:8px;border:1px solid rgba(128,128,128,.12);background:transparent;color:var(--text-secondary,#666);font-size:12px;cursor:pointer}
.feed-compose-tool:hover{border-color:var(--tapp-primary,#6366f1);color:var(--tapp-primary,#6366f1)}
.feed-compose-submit{display:flex;gap:8px;margin-left:auto}
.feed-compose-cancel{padding:7px 14px;border-radius:8px;border:1px solid rgba(128,128,128,.15);background:transparent;color:var(--text-secondary,#666);font-size:13px;cursor:pointer}
.feed-compose-publish{padding:7px 16px;border-radius:8px;border:none;background:var(--tapp-primary,#6366f1);color:#fff;font-size:13px;font-weight:600;cursor:pointer}
.feed-compose-publish:disabled,.feed-compose-cancel:disabled,.feed-compose-tool:disabled{opacity:.5;cursor:not-allowed}
.feed-item-media{margin-top:10px}
.feed-item-media-single{display:block}
.feed-item-media-single img,.feed-item-media-single video{display:block;width:100%;max-height:320px;border-radius:14px;object-fit:cover;background:rgba(128,128,128,.06)}
.feed-item-media-grid{display:grid;grid-template-columns:1fr 1fr;gap:4px;border-radius:14px;overflow:hidden}
.feed-media-cell{position:relative;aspect-ratio:1;background:rgba(128,128,128,.08);overflow:hidden}
.feed-media-cell img,.feed-media-cell video{width:100%;height:100%;object-fit:cover;display:block}
.feed-main-heading{min-width:0;display:flex;flex-direction:column;gap:2px}
.feed-section-title{font-size:16px;font-weight:750;color:var(--text-primary,#0f1419);line-height:1.2}
.feed-section-meta{min-height:15px;font-size:11px;font-weight:500;color:var(--text-secondary,#536471);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.feed-header-actions{display:flex;align-items:center;gap:8px;flex-shrink:0}
.feed-refresh-btn{width:34px;height:34px;border:none;border-radius:10px;background:rgba(128,128,128,.07);color:var(--text-secondary,#536471);cursor:pointer;display:flex;align-items:center;justify-content:center;transition:background .15s,color .15s,transform .15s;flex-shrink:0}
.feed-refresh-btn:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.11);color:var(--tapp-primary,#6366f1)}
.feed-refresh-btn:active{transform:scale(.96)}
.feed-refresh-btn:disabled{opacity:.65;cursor:default;transform:none}
.feed-refresh-loading svg{animation:aroSpin .8s linear infinite}
@keyframes aroSpin{to{transform:rotate(360deg)}}
/* Feed Mobile Tabs (hidden on desktop) */
.feed-mobile-tabs{display:none;border-bottom:1px solid rgba(128,128,128,.08);flex-shrink:0;background:rgba(255,255,255,.45)}
.feed-mobile-tab{flex:1;padding:12px 4px;border:none;background:none;font-size:13px;font-weight:500;color:var(--text-secondary,#536471);cursor:pointer;text-align:center;transition:all .15s;border-bottom:2px solid transparent}
.feed-mobile-tab:hover{background:rgba(128,128,128,.04)}
.feed-mobile-tab-active{color:var(--text-primary,#0f1419)!important;font-weight:700;border-bottom-color:var(--tapp-primary,#6366f1)!important}
.feed-mobile-compose{width:44px;border:none;background:none;color:var(--tapp-primary,#6366f1);display:flex;align-items:center;justify-content:center;cursor:pointer;border-bottom:2px solid transparent;flex-shrink:0}
.feed-mobile-compose:hover{background:rgba(99,102,241,.08)}
.feed-mobile-refresh{width:44px;border:none;background:none;color:var(--text-secondary,#536471);display:flex;align-items:center;justify-content:center;cursor:pointer;border-bottom:2px solid transparent}
.feed-mobile-refresh:hover{background:rgba(128,128,128,.04);color:var(--tapp-primary,#6366f1)}
.feed-mobile-refresh:disabled{opacity:.55;cursor:default}
.feed-mobile-refresh.feed-refresh-loading svg{animation:aroSpin .8s linear infinite}
/* Follow bar */
.feed-follow-bar{display:flex;gap:8px;padding:10px 16px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0;background:rgba(128,128,128,.02)}
.feed-follow-input{flex:1;height:36px;padding:0 14px;border-radius:12px;border:1px solid rgba(128,128,128,.15);background:rgba(128,128,128,.04);color:var(--text-primary,#0f1419);font-size:13px;outline:none;transition:border-color .2s,background .2s}
.feed-follow-input:focus{border-color:rgba(var(--tapp-primary-rgb,100,100,255),.5)}
.feed-follow-input::placeholder{color:var(--text-secondary,#bbb)}
.feed-follow-btn{padding:0 16px;height:36px;border-radius:12px;border:none;background:var(--tapp-primary,#6366f1);color:#fff;font-size:13px;font-weight:700;cursor:pointer;white-space:nowrap;transition:opacity .15s}
.feed-follow-btn:hover{opacity:.85}
.feed-follow-btn:disabled{opacity:.5;cursor:not-allowed}
/* Feed content / empty */
.feed-content{flex:1;min-height:0}
.feed-empty{min-height:260px;padding:48px 16px;text-align:center;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:8px;color:var(--text-secondary,#536471);font-size:13px}
.feed-main.feed-empty-visible .feed-content{display:none}
.feed-main.feed-empty-visible .feed-empty{flex:1;min-height:0}
.aro-empty-mark{width:44px;height:44px;border-radius:13px;background:rgba(128,128,128,.06);color:var(--text-secondary,#536471);display:flex;align-items:center;justify-content:center;flex-shrink:0}
.aro-empty-mark svg{width:21px;height:21px}
.feed-empty-title{font-size:14px;font-weight:700;color:var(--text-primary,#0f1419)}
.feed-empty-error .aro-empty-mark{background:rgba(239,68,68,.08);color:#ef4444}
.feed-empty-error #feed-empty-text{color:#ef4444}
.feed-skeleton-item{display:flex;gap:10px;padding:14px 16px;border-bottom:1px solid rgba(128,128,128,.06)}
.feed-skeleton-avatar{width:40px;height:40px;border-radius:50%;flex-shrink:0;background:linear-gradient(90deg,rgba(128,128,128,.08),rgba(128,128,128,.16),rgba(128,128,128,.08));background-size:200% 100%;animation:aroSkeleton 1.1s ease-in-out infinite}
.feed-skeleton-body{flex:1;min-width:0;display:flex;flex-direction:column;gap:8px;padding-top:3px}
.feed-skeleton-line{height:10px;border-radius:999px;background:linear-gradient(90deg,rgba(128,128,128,.08),rgba(128,128,128,.16),rgba(128,128,128,.08));background-size:200% 100%;animation:aroSkeleton 1.1s ease-in-out infinite}
.feed-skeleton-line-short{width:34%}
.feed-skeleton-line-mid{width:62%}
@keyframes aroSkeleton{0%{background-position:200% 0}100%{background-position:-200% 0}}

/* ===== Feed Items (tweet-like cards) ===== */
.feed-item{display:flex;gap:10px;padding:14px 16px;border-bottom:1px solid rgba(128,128,128,.06);transition:background .12s;cursor:default}
.feed-item:hover{background:rgba(128,128,128,.03)}
.feed-item-avatar{width:40px;height:40px;border-radius:50%;flex-shrink:0;overflow:hidden;display:flex;align-items:center;justify-content:center;font-size:15px;font-weight:600;background:rgba(var(--tapp-primary-rgb,128,128,128),.1);color:var(--tapp-primary,#888)}
.feed-item-avatar img{width:100%;height:100%;object-fit:cover}
.feed-item-icon{width:40px;height:40px;border-radius:10px;flex-shrink:0;display:flex;align-items:center;justify-content:center;font-size:18px;background:rgba(var(--tapp-primary-rgb,128,128,128),.06)}
.feed-item-body{flex:1;min-width:0}
.feed-item-header{display:flex;align-items:center;gap:6px;flex-wrap:wrap}
.feed-item-name{font-size:14px;font-weight:700;color:var(--text-primary,#1a1a1a);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.feed-item-handle{font-size:13px;color:var(--text-secondary,#999);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.feed-item-sep{font-size:13px;color:var(--text-secondary,#bbb)}
.feed-item-time{font-size:13px;color:var(--text-secondary,#999);white-space:nowrap;flex-shrink:0}
.feed-item-text{font-size:14px;line-height:1.55;color:var(--text-primary,#1a1a1a);margin-top:4px;white-space:pre-wrap;overflow-wrap:break-word}
.feed-item-badges{display:flex;gap:5px;flex-wrap:wrap;margin-top:6px}
.feed-item-actions{display:flex;gap:24px;margin-top:8px}
.feed-item-action{display:flex;align-items:center;gap:4px;background:none;border:none;color:var(--text-secondary,#999);font-size:12px;cursor:pointer;padding:4px 0;border-radius:4px;transition:color .15s}
.feed-item-action:hover{color:var(--tapp-primary,#6366f1)}
.feed-item-action-danger:hover{color:#ef4444}

/* ===== Aro Badges ===== */
.aro-badge{font-size:10px;padding:2px 8px;border-radius:8px;font-weight:500;white-space:nowrap}
.aro-badge-type{background:rgba(var(--tapp-primary-rgb,100,100,255),.08);color:var(--tapp-primary,#6366f1)}
.aro-badge-status{background:rgba(34,197,94,.1);color:#22c55e}
.aro-badge-pending{background:rgba(245,158,11,.1);color:#f59e0b}
.aro-badge-vis{background:rgba(var(--tapp-primary-rgb,100,100,255),.06);color:var(--tapp-primary,#888)}
.aro-unread-dot{width:7px;height:7px;border-radius:50%;background:var(--tapp-primary,#6366f1);flex-shrink:0;box-shadow:0 0 4px rgba(var(--tapp-primary-rgb,100,100,255),.4)}

/* ===== Dark Mode Overrides for Aro ===== */
.dark .aro-nav{background:rgba(10,10,10,.78)}
.dark .feed-sidebar{border-color:rgba(255,255,255,.06)}
.dark .feed-display-name{color:rgba(255,255,255,.92)}
.dark .feed-nav-item{color:rgba(255,255,255,.85)}
.dark .feed-nav-active{color:rgba(255,255,255,.95)!important}
.dark .feed-sidebar-footer{border-color:rgba(255,255,255,.06)}
.dark .feed-main-header{background:rgba(10,10,10,.52);border-color:rgba(255,255,255,.06)}
.dark .feed-section-title{color:rgba(255,255,255,.92)}
.dark .feed-section-meta{color:rgba(255,255,255,.45)}
.dark .feed-refresh-btn{background:rgba(255,255,255,.06);color:rgba(255,255,255,.56)}
.dark .feed-refresh-btn:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.16);color:var(--tapp-primary,#818cf8)}
.dark .feed-mobile-tabs{background:rgba(10,10,10,.5)}
.dark .feed-mobile-tab-active{color:rgba(255,255,255,.95)!important}
.dark .feed-mobile-refresh{color:rgba(255,255,255,.5)}
.dark .feed-mobile-refresh:hover{background:rgba(255,255,255,.04);color:var(--tapp-primary,#818cf8)}
.dark .feed-stat-num{color:rgba(255,255,255,.92)}
.dark .feed-sidebar-stat{background:rgba(255,255,255,.045)}
.dark .feed-mobile-stat{background:rgba(255,255,255,.035);color:rgba(255,255,255,.48)}
.dark .feed-profile-card{border-color:rgba(255,255,255,.08);background:rgba(255,255,255,.035)}
.dark .feed-profile-toggle{background:rgba(255,255,255,.045);color:rgba(255,255,255,.48)}
.dark .feed-identity-actor{background:rgba(255,255,255,.06);color:rgba(255,255,255,.68)}
.dark .feed-item-name{color:rgba(255,255,255,.92)}
.dark .feed-item-text{color:rgba(255,255,255,.85)}
.dark .feed-item:hover{background:rgba(255,255,255,.03)}
.dark .feed-follow-input{background:rgba(255,255,255,.03);border-color:rgba(255,255,255,.1);color:rgba(255,255,255,.9)}
.dark .aro-empty-mark{background:rgba(255,255,255,.055);color:rgba(255,255,255,.42)}
.dark .feed-empty-title{color:rgba(255,255,255,.86)}

/* ===== Responsive for Aro Nav ===== */
@media(max-width:768px){
  .aro-nav-item span{display:none}
  .aro-nav-item{padding:8px 12px}
  .aro-nav{justify-content:center;gap:4px}
  .feed-sidebar{display:none}
  .feed-main-header{display:none}
  .feed-mobile-tabs{display:flex}
  .feed-profile-mobile{display:flex}
  .feed-mobile-stats{display:grid}
  .feed-main{max-width:none}
}

/* ===== Layout ===== */
.messenger-app{display:flex;flex:1;min-height:0;overflow:hidden}
.sidebar{display:flex;flex-direction:column;width:280px;border-right:1px solid rgba(128,128,128,.08);flex-shrink:0;overflow:hidden}
.sidebar-header{padding:14px 16px 10px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0;display:flex;align-items:center;justify-content:space-between}
.sidebar-title{margin:0;font-size:15px;font-weight:600;color:var(--text-primary,#1a1a1a);letter-spacing:-.01em}
.create-btn{width:28px;height:28px;border-radius:50%;border:none;background:var(--tapp-primary,#888);color:#fff;font-size:18px;font-weight:300;cursor:pointer;display:flex;align-items:center;justify-content:center;transition:opacity .15s;flex-shrink:0;line-height:1}
.create-btn:hover{opacity:.85}
.conv-list{flex:1;min-height:0;overflow-y:auto;padding:8px;display:flex;flex-direction:column;gap:6px}
.chat-main{flex:1;min-width:0;display:flex;flex-direction:column;position:relative;overflow:hidden}
.member-panel{display:flex;flex-direction:column;width:220px;border-left:1px solid rgba(128,128,128,.08);flex-shrink:0;overflow:hidden;transition:width .2s}
.member-panel.member-collapsed{width:0;border-left:none;overflow:hidden}
.member-back-btn{display:none;background:none;border:none;font-size:16px;color:var(--text-primary,#1a1a1a);cursor:pointer;padding:2px 4px;margin-right:6px;border-radius:6px;flex-shrink:0}
.member-back-btn:hover{background:rgba(128,128,128,.08)}
.member-header{padding:12px 14px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0;display:flex;align-items:center;justify-content:space-between}
.member-title{margin:0;font-size:12px;font-weight:600;color:var(--text-secondary,#888);text-transform:uppercase;letter-spacing:.04em}
.member-list{flex:1;overflow-y:auto;padding:8px}
/* Invite popover */
.invite-wrap{position:relative}
.invite-toggle{width:24px;height:24px;border:none;background:none;color:var(--tapp-primary,#6366f1);cursor:pointer;display:flex;align-items:center;justify-content:center;border-radius:6px;transition:background .15s}
.invite-toggle:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.1)}
.invite-popover{position:fixed;width:240px;background:var(--bg-primary,#fff);border:1px solid rgba(128,128,128,.1);border-radius:12px;box-shadow:0 8px 24px rgba(0,0,0,.12);z-index:200;overflow:hidden}
.invite-pop-section{padding:8px}
.invite-pop-label{font-size:10px;font-weight:600;color:var(--text-secondary,#888);text-transform:uppercase;letter-spacing:.04em;padding:4px 6px 6px}
.invite-pop-list{max-height:180px;overflow-y:auto}
.invite-pop-empty{padding:12px 6px;text-align:center;font-size:11px;color:var(--text-secondary,#999)}
.invite-pop-contact{display:flex;align-items:center;gap:8px;width:100%;padding:6px 8px;border:none;background:none;border-radius:8px;cursor:pointer;transition:background .12s;text-align:left}
.invite-pop-contact:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.06)}
.invite-pop-contact-avatar{width:28px;height:28px;border-radius:50%;background:rgba(var(--tapp-primary-rgb,128,128,128),.1);color:var(--tapp-primary,#888);display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;flex-shrink:0;overflow:hidden}
.invite-pop-contact-avatar img{width:100%;height:100%;object-fit:cover}
.invite-pop-contact-info{min-width:0;flex:1}
.invite-pop-contact-name{font-size:12px;font-weight:500;color:var(--text-primary,#1a1a1a);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.invite-pop-contact-url{font-size:10px;color:var(--text-secondary,#999);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.invite-pop-contact-added{font-size:10px;color:var(--tapp-primary,#6366f1);flex-shrink:0}
.invite-pop-divider{height:1px;background:rgba(128,128,128,.08);margin:0 8px}
.invite-pop-manual{display:flex;gap:6px;padding:0 2px}
.invite-pop-send{width:28px;height:28px;border:none;border-radius:8px;background:var(--tapp-primary,#6366f1);color:#fff;cursor:pointer;display:flex;align-items:center;justify-content:center;flex-shrink:0;transition:opacity .15s}
.invite-pop-send:hover{opacity:.85}
.invite-pop-send:disabled{opacity:.4;cursor:not-allowed}
.dark .invite-popover{background:var(--bg-primary,#1a1a1a);border-color:rgba(255,255,255,.08);box-shadow:0 8px 24px rgba(0,0,0,.35)}
.dark .invite-pop-contact-name{color:rgba(255,255,255,.9)}
.invite-bar{display:flex;gap:6px;padding:8px 10px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0}
.invite-input{flex:1;padding:5px 8px;border:1px solid rgba(128,128,128,.15);border-radius:6px;background:rgba(128,128,128,.04);color:var(--text-primary,#fff);font-size:12px;outline:none}
.invite-input:focus{border-color:rgba(var(--tapp-primary-rgb,100,100,255),.5)}
.invite-btn{padding:5px 10px;border:none;border-radius:6px;background:var(--tapp-primary,#6366f1);color:#fff;font-size:12px;cursor:pointer;white-space:nowrap}
.invite-btn:hover{opacity:.85}
.invite-btn:disabled{opacity:.5;cursor:not-allowed}

/* ===== Conversation Items ===== */
.conv-item{position:relative;display:flex;align-items:center;gap:12px;width:100%;padding:12px 12px 12px 14px;border:1px solid transparent;border-radius:14px;background:rgba(128,128,128,.04);cursor:pointer;text-align:left;transition:background .15s,border-color .15s,box-shadow .15s;overflow:hidden}
.conv-item:hover{background:rgba(var(--tapp-primary-rgb,99,102,241),.08);border-color:rgba(var(--tapp-primary-rgb,99,102,241),.12)}
.conv-active{background:rgba(var(--tapp-primary-rgb,99,102,241),.14)!important;border-color:rgba(var(--tapp-primary-rgb,99,102,241),.28)!important;box-shadow:0 1px 0 rgba(var(--tapp-primary-rgb,99,102,241),.08)}
.conv-accent{position:absolute;left:0;top:10px;bottom:10px;width:3px;border-radius:0 3px 3px 0;background:transparent;transition:background .15s}
.conv-active .conv-accent,.conv-unread .conv-accent{background:var(--tapp-primary,#6366f1)}
.conv-avatar{width:42px;height:42px;border-radius:12px;display:flex;align-items:center;justify-content:center;font-size:15px;font-weight:600;flex-shrink:0;overflow:hidden}
.conv-avatar img{width:100%;height:100%;object-fit:cover}
.avatar-channel{background:rgba(var(--tapp-primary-rgb,99,102,241),.14);color:var(--tapp-primary,#6366f1)}
.avatar-room{background:rgba(var(--tapp-primary-rgb,99,102,241),.14);color:var(--tapp-primary,#6366f1)}
.conv-info{min-width:0;flex:1;display:flex;flex-direction:column;gap:3px}
.conv-top{display:flex;align-items:center;gap:8px;min-width:0}
.conv-name{flex:1;min-width:0;font-size:13.5px;font-weight:600;color:var(--text-primary,#1a1a1a);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.conv-time{flex-shrink:0;font-size:11px;font-weight:500;color:var(--text-secondary,#999);white-space:nowrap}
.conv-bottom{display:flex;align-items:center;gap:6px;min-width:0}
.conv-preview{flex:1;min-width:0;font-size:12px;color:var(--text-secondary,#888);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.conv-unread .conv-preview{color:var(--text-primary,#333);font-weight:500}
.conv-badge{min-width:18px;height:18px;padding:0 5px;border-radius:9px;background:var(--tapp-primary,#6366f1);color:#fff;font-size:10px;font-weight:700;display:flex;align-items:center;justify-content:center;flex-shrink:0}
.conv-closed{font-size:10px;color:var(--text-secondary,#999);background:rgba(128,128,128,.08);padding:2px 6px;border-radius:6px;flex-shrink:0}
.conv-pending{font-size:10px;color:#f59e0b;background:rgba(245,158,11,.1);padding:2px 6px;border-radius:6px;flex-shrink:0}
.conv-empty{padding:40px 16px;text-align:center;font-size:13px;color:var(--text-secondary,#999)}
.conv-empty-fill{flex:1;min-height:0;display:flex;align-items:center;justify-content:center;padding:0 16px}
.dark .conv-item{background:rgba(255,255,255,.04)}
.dark .conv-item:hover{background:rgba(var(--tapp-primary-rgb,99,102,241),.14)}
.dark .conv-active{background:rgba(var(--tapp-primary-rgb,99,102,241),.2)!important}
.dark .conv-name{color:rgba(255,255,255,.92)}
.dark .conv-preview{color:rgba(255,255,255,.55)}
.dark .conv-unread .conv-preview{color:rgba(255,255,255,.82)}

/* ===== Empty State ===== */
.empty-state{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:var(--text-secondary,#999)}
.empty-icon{width:44px;height:44px;border-radius:13px;background:rgba(var(--tapp-primary-rgb,128,128,128),.06);display:flex;align-items:center;justify-content:center;font-size:0;margin-bottom:10px;color:var(--text-secondary,#999)}
.empty-icon svg{width:21px;height:21px;stroke-width:1.8}
.empty-text{font-size:13px;font-weight:400;color:var(--text-secondary,#999)}

/* ===== Chat Container ===== */
.chat-container{flex:1;min-height:0;display:flex;flex-direction:column;position:relative}
.chat-header{display:flex;align-items:center;gap:10px;padding:10px 14px;border-bottom:1px solid rgba(128,128,128,.06);flex-shrink:0}
.back-btn{display:none;background:none;border:none;font-size:16px;padding:4px 8px;border-radius:8px;cursor:pointer;color:var(--text-secondary,#888)}
.chat-hdr-avatar{width:32px;height:32px;border-radius:50%;overflow:hidden;flex-shrink:0;display:flex;align-items:center;justify-content:center;font-size:13px;font-weight:600;background:rgba(var(--tapp-primary-rgb,128,128,128),.1);color:var(--tapp-primary,#888)}
.chat-hdr-avatar img{width:100%;height:100%;object-fit:cover}
.chat-header-info{min-width:0;flex:1}
.chat-name{font-size:14px;font-weight:600;color:var(--text-primary,#1a1a1a);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.chat-meta{display:flex;gap:6px;align-items:center;margin-top:2px;flex-wrap:wrap}
.meta-badge{font-size:10px;padding:2px 8px;border-radius:10px;background:rgba(var(--tapp-primary-rgb,128,128,128),.08);color:var(--tapp-primary,#888)}
.badge-channel{}
.badge-room{}
.badge-role{}
.badge-closed{background:rgba(128,128,128,.08);color:var(--text-secondary,#999)}
.badge-pending{background:rgba(245,158,11,.1);color:#f59e0b}
.chat-actions{display:flex;gap:6px;flex-shrink:0;align-items:center}
.member-toggle-btn{width:28px;height:28px;border-radius:8px;border:none;background:rgba(128,128,128,.06);color:var(--text-secondary,#999);cursor:pointer;display:flex;align-items:center;justify-content:center;transition:background .15s}
.member-toggle-btn:hover{background:rgba(128,128,128,.12)}
.action-btn{font-size:11px;padding:4px 10px;border-radius:10px;border:none;cursor:pointer;transition:background .15s}
.action-accept{color:#fff;background:#22c55e}
.action-accept:hover{background:#16a34a}
.manage-wrap{position:relative}
.manage-btn{width:28px;height:28px;border-radius:8px;border:none;background:rgba(128,128,128,.06);color:var(--text-secondary,#999);font-size:16px;cursor:pointer;display:flex;align-items:center;justify-content:center;transition:background .15s;line-height:1}
.manage-btn:hover{background:rgba(128,128,128,.12)}
.manage-dropdown{display:none;position:absolute;right:0;top:calc(100% + 4px);min-width:120px;background:var(--bg-primary,#fff);border:1px solid rgba(128,128,128,.1);border-radius:10px;box-shadow:0 4px 16px rgba(0,0,0,.1);z-index:50;padding:4px;overflow:hidden}
.manage-dropdown.open{display:block}
.manage-item{display:flex;align-items:center;gap:8px;width:100%;padding:7px 10px;border:none;background:none;border-radius:7px;font-size:12px;color:var(--text-primary,#333);cursor:pointer;transition:background .12s;text-align:left}
.manage-item:hover{background:rgba(128,128,128,.08)}
.manage-item-danger{color:#ef4444}
.manage-item-danger:hover{background:rgba(239,68,68,.06)}
.dark .manage-dropdown{background:var(--bg-primary,#1a1a1a);border-color:rgba(255,255,255,.08);box-shadow:0 4px 16px rgba(0,0,0,.3)}

/* ===== Pinned Bar ===== */
.pinned-bar{display:flex;align-items:center;gap:8px;padding:6px 14px;background:rgba(var(--tapp-primary-rgb,99,102,241),.06);border-bottom:1px solid rgba(var(--tapp-primary-rgb,99,102,241),.1);cursor:pointer;flex-shrink:0;min-height:0;overflow:hidden;transition:background .15s}
.pinned-bar:hover{background:rgba(var(--tapp-primary-rgb,99,102,241),.1)}
.pinned-bar-icon{width:14px;height:14px;flex-shrink:0;color:var(--tapp-primary,#6366f1);display:flex;align-items:center;justify-content:center}
.pinned-bar-body{flex:1;min-width:0;display:flex;flex-direction:column;gap:1px}
.pinned-bar-label{font-size:10px;font-weight:600;color:var(--tapp-primary,#6366f1);text-transform:uppercase;letter-spacing:.3px}
.pinned-bar-text{font-size:12px;color:var(--text-primary,#333);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.pinned-bar-close{width:20px;height:20px;border:none;background:none;color:var(--text-secondary,#999);cursor:pointer;display:flex;align-items:center;justify-content:center;border-radius:4px;flex-shrink:0;font-size:14px;line-height:1}
.pinned-bar-close:hover{background:rgba(128,128,128,.1)}

/* ===== Messages ===== */
.messages-area{flex:1;overflow-y:auto;padding:16px;padding-bottom:72px;display:flex;flex-direction:column;gap:8px}
.messages-empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:var(--text-secondary,#999)}
.messages-empty-icon{width:44px;height:44px;border-radius:13px;background:rgba(var(--tapp-primary-rgb,128,128,128),.06);display:flex;align-items:center;justify-content:center;font-size:0;margin-bottom:10px}
.messages-empty-icon svg{width:21px;height:21px;stroke-width:1.8}
.msg-day-sep{display:flex;align-items:center;justify-content:center;margin:10px 0 4px;user-select:none}
.msg-day-label{font-size:11px;font-weight:600;color:var(--text-secondary,#888);background:rgba(128,128,128,.1);padding:4px 12px;border-radius:999px;letter-spacing:.2px}
.msg-row{display:flex;gap:8px;align-items:flex-end;position:relative}
.msg-row.msg-compact{margin-top:-4px;gap:8px}
.msg-local{justify-content:flex-end;padding-left:48px}
.msg-remote{justify-content:flex-start;padding-right:48px}
.msg-avatar{width:28px;height:28px;border-radius:50%;overflow:hidden;flex-shrink:0;display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;background:rgba(var(--tapp-primary-rgb,128,128,128),.12);color:var(--tapp-primary,#888);margin-bottom:2px}
.msg-avatar img{width:100%;height:100%;object-fit:cover}
.msg-avatar-spacer{width:28px;flex-shrink:0}
.msg-bubble{position:relative;max-width:72%;padding:8px 12px;border-radius:16px;font-size:13px;line-height:1.5}
.bubble-local{background:var(--tapp-primary,#6366f1);color:#fff;border-bottom-right-radius:6px}
.bubble-remote{background:rgba(128,128,128,.07);color:var(--text-primary,#1a1a1a);border-bottom-left-radius:6px}
.msg-more-btn{position:absolute;top:4px;width:24px;height:24px;border:none;border-radius:8px;background:rgba(0,0,0,.06);color:inherit;opacity:0;pointer-events:none;cursor:pointer;display:flex;align-items:center;justify-content:center;transition:opacity .12s,background .12s;z-index:2}
.msg-local .msg-more-btn{left:-30px}
.msg-remote .msg-more-btn{right:-30px}
.msg-row:hover .msg-more-btn,.msg-more-btn:focus-visible{opacity:1;pointer-events:auto}
.msg-more-btn:hover{background:rgba(0,0,0,.12)}
.bubble-local .msg-more-btn{background:rgba(255,255,255,.18);color:#fff}
.bubble-local .msg-more-btn:hover{background:rgba(255,255,255,.28)}
.msg-sender{font-size:11px;font-weight:500;color:var(--tapp-primary,#888);margin-bottom:2px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.msg-text{white-space:pre-wrap;overflow-wrap:break-word}
.msg-footer{display:flex;align-items:center;gap:4px;margin-top:4px}
.msg-local .msg-footer{justify-content:flex-end}
.msg-remote .msg-footer{justify-content:flex-start}
.msg-pin{font-size:10px}
.msg-time{font-size:10px;opacity:.45}
.msg-compact .msg-footer{margin-top:2px}
.dark .msg-day-label{background:rgba(255,255,255,.08);color:rgba(255,255,255,.6)}
.dark .msg-more-btn{background:rgba(255,255,255,.1)}
.dark .msg-more-btn:hover{background:rgba(255,255,255,.18)}

/* ===== Input ===== */
.input-float-wrap{position:absolute;bottom:0;left:0;right:0;z-index:20;padding:8px 12px;padding-bottom:calc(8px + env(safe-area-inset-bottom,0px));background:linear-gradient(to top,var(--bg-primary,#fff) 70%,transparent);pointer-events:none}
.input-float-wrap>*{pointer-events:auto}
.input-bar{display:flex;align-items:flex-end;gap:8px;padding:8px 10px;background:var(--bg-primary,#fff);border:1px solid rgba(128,128,128,.12);border-radius:18px;box-shadow:0 2px 12px rgba(0,0,0,.06)}
.attach-btn{width:32px;height:32px;border-radius:50%;border:none;background:rgba(128,128,128,.06);color:var(--text-secondary,#999);cursor:pointer;display:flex;align-items:center;justify-content:center;flex-shrink:0;transition:all .15s;margin-bottom:2px}
.attach-btn:hover{background:rgba(var(--tapp-primary-rgb,100,100,255),.1);color:var(--tapp-primary,#6366f1)}
.attach-btn.attach-btn-active{background:rgba(var(--tapp-primary-rgb,100,100,255),.12);color:var(--tapp-primary,#6366f1);transform:rotate(45deg)}
.msg-input{flex:1;min-height:20px;padding:6px 0;border:none;background:transparent;color:var(--text-primary,#1a1a1a);font-size:13px;line-height:1.45;outline:none;resize:none;overflow-y:hidden;font-family:inherit}
.msg-input::placeholder{color:var(--text-secondary,#bbb)}
.send-btn{width:32px;height:32px;border-radius:50%;border:none;background:rgba(128,128,128,.08);color:var(--text-secondary,#999);cursor:pointer;display:flex;align-items:center;justify-content:center;flex-shrink:0;transition:all .2s;margin-bottom:2px}
.send-btn:hover{color:var(--tapp-primary,#6366f1);background:rgba(var(--tapp-primary-rgb,100,100,255),.12)}
.send-btn:active{transform:translateY(-1px);color:var(--tapp-primary,#6366f1)}
.send-btn:disabled{opacity:.35;cursor:not-allowed;transform:none;background:rgba(128,128,128,.06);color:var(--text-secondary,#bbb)}
.send-btn.send-ready{background:var(--tapp-primary,#6366f1);color:#fff;box-shadow:0 2px 8px rgba(var(--tapp-primary-rgb,99,102,241),.35)}
.send-btn.send-ready:hover{background:var(--tapp-primary,#6366f1);color:#fff;filter:brightness(1.06)}
.send-btn.send-ready:active{transform:scale(.96)}
.dark .send-btn{background:rgba(255,255,255,.08)}
.dark .send-btn.send-ready{background:var(--tapp-primary,#6366f1);color:#fff}
.dark .input-float-wrap{background:linear-gradient(to top,var(--bg-primary,#1a1a1a) 70%,transparent)}
.dark .input-bar{background:var(--bg-primary,#1a1a1a);border-color:rgba(255,255,255,.1);box-shadow:0 2px 12px rgba(0,0,0,.2)}

/* ===== Attachment Menu ===== */
.attach-menu{position:absolute;bottom:calc(100% + 6px);left:0;background:var(--bg-primary,#fff);border:1px solid rgba(128,128,128,.1);border-radius:14px;box-shadow:0 6px 20px rgba(0,0,0,.1);z-index:60;padding:6px;display:grid;grid-template-columns:repeat(4,1fr);gap:2px;min-width:240px}
.attach-menu-item{display:flex;flex-direction:column;align-items:center;gap:5px;padding:10px 6px;border:none;background:none;border-radius:10px;cursor:pointer;transition:background .12s;color:var(--text-primary,#1a1a1a);font-size:10px;font-weight:500;white-space:nowrap}
.attach-menu-item:hover{background:rgba(128,128,128,.06)}
.attach-menu-icon{width:36px;height:36px;border-radius:10px;display:flex;align-items:center;justify-content:center;font-size:18px}
.attach-icon-image{background:rgba(59,130,246,.1);color:#3b82f6}
.attach-icon-file{background:rgba(245,158,11,.1);color:#f59e0b}
.attach-icon-tapp{background:rgba(var(--tapp-primary-rgb,100,100,255),.1);color:var(--tapp-primary,#6366f1)}
.attach-icon-brew{background:rgba(34,197,94,.1);color:#22c55e}
.attach-icon-library{background:rgba(168,85,247,.1);color:#a855f7}
.attach-icon-report{background:rgba(239,68,68,.1);color:#ef4444}
.dark .attach-menu{background:var(--bg-primary,#1a1a1a);border-color:rgba(255,255,255,.08);box-shadow:0 6px 20px rgba(0,0,0,.35)}
.dark .attach-menu-item{color:rgba(255,255,255,.9)}

/* ===== Attachment Preview ===== */
.attach-preview{padding:8px 10px;display:flex;align-items:center;gap:10px;background:var(--bg-primary,#fff);border:1px solid rgba(128,128,128,.1);border-bottom:none;border-radius:14px 14px 0 0;margin-bottom:-1px}
.attach-preview-thumb{width:48px;height:48px;border-radius:8px;overflow:hidden;flex-shrink:0;display:flex;align-items:center;justify-content:center;background:rgba(128,128,128,.06)}
.attach-preview-thumb img{width:100%;height:100%;object-fit:cover}
.attach-preview-icon{width:48px;height:48px;border-radius:8px;display:flex;align-items:center;justify-content:center;font-size:22px;flex-shrink:0}
.attach-preview-info{flex:1;min-width:0}
.attach-preview-name{font-size:12px;font-weight:500;color:var(--text-primary,#1a1a1a);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.attach-preview-meta{font-size:10px;color:var(--text-secondary,#999);margin-top:2px}
.attach-preview-remove{width:24px;height:24px;border-radius:50%;border:none;background:rgba(128,128,128,.08);color:var(--text-secondary,#999);cursor:pointer;display:flex;align-items:center;justify-content:center;flex-shrink:0;transition:background .12s;font-size:14px;line-height:1}
.attach-preview-remove:hover{background:rgba(239,68,68,.1);color:#ef4444}

/* ===== Content Picker Overlay ===== */
.picker-overlay{position:fixed;top:0;left:0;right:0;bottom:0;z-index:200;display:flex;align-items:flex-end;justify-content:center;background:rgba(0,0,0,.35);animation:pickerFadeIn .18s ease}
@keyframes pickerFadeIn{from{opacity:0}to{opacity:1}}
@keyframes pickerSlideUp{from{transform:translateY(20px);opacity:0}to{transform:translateY(0);opacity:1}}
.picker-sheet{width:100%;max-width:480px;max-height:70vh;background:var(--bg-primary,#fff);border-radius:16px 16px 0 0;display:flex;flex-direction:column;overflow:hidden;animation:pickerSlideUp .2s ease}
.picker-header{display:flex;align-items:center;gap:10px;padding:14px 16px 10px;border-bottom:1px solid rgba(128,128,128,.08);flex-shrink:0}
.picker-header-icon{width:32px;height:32px;border-radius:8px;display:flex;align-items:center;justify-content:center;font-size:16px;flex-shrink:0}
.picker-header-title{flex:1;font-size:15px;font-weight:600;color:var(--text-primary,#1a1a1a)}
.picker-close-btn{width:28px;height:28px;border-radius:50%;border:none;background:rgba(128,128,128,.08);color:var(--text-secondary,#999);cursor:pointer;display:flex;align-items:center;justify-content:center;font-size:16px;flex-shrink:0;transition:background .12s}
.picker-close-btn:hover{background:rgba(128,128,128,.15)}
.picker-search{padding:8px 16px;flex-shrink:0}
.picker-search input{width:100%;padding:8px 12px;border:1px solid rgba(128,128,128,.12);border-radius:10px;background:rgba(128,128,128,.04);font-size:13px;color:var(--text-primary,#1a1a1a);outline:none;box-sizing:border-box}
.picker-search input:focus{border-color:var(--tapp-primary,#6366f1);background:transparent}
.picker-body{flex:1;overflow-y:auto;padding:4px 8px 8px}
.picker-loading,.picker-empty{display:flex;flex-direction:column;align-items:center;justify-content:center;padding:32px 16px;color:var(--text-secondary,#999);font-size:13px;gap:8px}
.picker-loading-spinner{width:24px;height:24px;border:2px solid rgba(128,128,128,.15);border-top-color:var(--tapp-primary,#6366f1);border-radius:50%;animation:pickerSpin .7s linear infinite}
@keyframes pickerSpin{to{transform:rotate(360deg)}}
.picker-item{display:flex;align-items:center;gap:10px;padding:10px 10px;border-radius:10px;cursor:pointer;transition:background .12s;border:none;background:none;width:100%;text-align:left;color:var(--text-primary,#1a1a1a);font-family:inherit}
.picker-item:hover{background:rgba(128,128,128,.06)}
.picker-item-icon{width:36px;height:36px;border-radius:8px;display:flex;align-items:center;justify-content:center;font-size:18px;flex-shrink:0;background:rgba(128,128,128,.06)}
.picker-item-body{flex:1;min-width:0}
.picker-item-name{font-size:13px;font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.picker-item-meta{font-size:11px;color:var(--text-secondary,#999);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;margin-top:1px}
.picker-item-check{width:20px;height:20px;border-radius:50%;border:1.5px solid rgba(128,128,128,.2);display:flex;align-items:center;justify-content:center;flex-shrink:0;transition:all .12s;font-size:12px;color:transparent}
.picker-item.selected .picker-item-check{border-color:var(--tapp-primary,#6366f1);background:var(--tapp-primary,#6366f1);color:#fff}
.picker-form{padding:12px 16px;display:flex;flex-direction:column;gap:10px}
.picker-form label{font-size:12px;font-weight:500;color:var(--text-secondary,#999);display:flex;flex-direction:column;gap:4px}
.picker-form input,.picker-form textarea{width:100%;padding:8px 12px;border:1px solid rgba(128,128,128,.12);border-radius:10px;background:rgba(128,128,128,.04);font-size:13px;color:var(--text-primary,#1a1a1a);outline:none;font-family:inherit;box-sizing:border-box}
.picker-form input:focus,.picker-form textarea:focus{border-color:var(--tapp-primary,#6366f1);background:transparent}
.picker-form textarea{min-height:60px;resize:vertical}
.picker-footer{display:flex;gap:8px;padding:10px 16px;border-top:1px solid rgba(128,128,128,.08);flex-shrink:0}
.picker-footer-btn{flex:1;padding:10px;border:none;border-radius:10px;font-size:13px;font-weight:500;cursor:pointer;transition:all .12s;font-family:inherit}
.picker-btn-cancel{background:rgba(128,128,128,.08);color:var(--text-primary,#1a1a1a)}
.picker-btn-cancel:hover{background:rgba(128,128,128,.14)}
.picker-btn-confirm{background:var(--tapp-primary,#6366f1);color:#fff}
.picker-btn-confirm:hover{filter:brightness(1.1)}
.picker-btn-confirm:disabled{opacity:.4;cursor:not-allowed;filter:none}
.picker-tabs{display:flex;gap:4px;padding:4px 14px 6px;flex-shrink:0;overflow-x:auto}
.picker-tab{padding:5px 12px;border:none;border-radius:8px;background:rgba(128,128,128,.06);color:var(--text-secondary,#999);font-size:12px;font-weight:500;cursor:pointer;transition:all .12s;white-space:nowrap;font-family:inherit}
.picker-tab.active{background:rgba(var(--tapp-primary-rgb,100,100,255),.1);color:var(--tapp-primary,#6366f1)}
.dark .picker-sheet{background:var(--bg-primary,#1a1a1a)}
.dark .picker-header{border-color:rgba(255,255,255,.06)}
.dark .picker-footer{border-color:rgba(255,255,255,.06)}
.dark .picker-item{color:rgba(255,255,255,.9)}
.dark .picker-form input,.dark .picker-form textarea{background:rgba(255,255,255,.04);border-color:rgba(255,255,255,.1);color:rgba(255,255,255,.9)}
.dark .picker-search input{background:rgba(255,255,255,.04);border-color:rgba(255,255,255,.1);color:rgba(255,255,255,.9)}
.dark .picker-btn-cancel{background:rgba(255,255,255,.08);color:rgba(255,255,255,.9)}

/* ===== Rich Message Bubbles ===== */

/* -- Message Context Menu -- */
.msg-ctx-menu{position:fixed;z-index:200;min-width:140px;background:var(--bg-primary,#fff);border-radius:12px;box-shadow:0 8px 32px rgba(0,0,0,.18);padding:4px;animation:ctxFadeIn .12s ease}
@keyframes ctxFadeIn{from{opacity:0;transform:scale(.95)}to{opacity:1;transform:scale(1)}}
.msg-ctx-item{display:flex;align-items:center;gap:8px;width:100%;padding:9px 12px;border:none;background:none;border-radius:8px;font-size:13px;font-weight:500;color:var(--text-primary,#1a1a1a);cursor:pointer;transition:background .12s;font-family:inherit}
.msg-ctx-item:hover{background:rgba(128,128,128,.08)}
.msg-ctx-item:active{background:rgba(128,128,128,.14)}
.msg-ctx-item svg{flex-shrink:0;opacity:.6}
.dark .msg-ctx-menu{background:var(--bg-primary,#1a1a1a);box-shadow:0 8px 32px rgba(0,0,0,.4)}
.dark .msg-ctx-item{color:rgba(255,255,255,.9)}

/* -- Quote Block inside message bubble -- */
.msg-quote-block{display:flex;gap:0;margin-bottom:6px;border-radius:8px;overflow:hidden;background:rgba(128,128,128,.08);padding:6px 8px}
.msg-quote-bar{width:3px;border-radius:2px;background:var(--tapp-primary,#6366f1);flex-shrink:0;margin-right:8px}
.msg-quote-content{min-width:0;flex:1}
.msg-quote-sender{font-size:11px;font-weight:600;opacity:.7;margin-bottom:1px}
.msg-quote-text{font-size:12px;opacity:.6;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:220px}
.bubble-local .msg-quote-block{background:rgba(255,255,255,.15)}
.bubble-local .msg-quote-bar{background:rgba(255,255,255,.6)}

/* -- Quote Preview above input bar -- */
.quote-preview{display:flex;align-items:center;gap:0;padding:8px 12px;background:rgba(128,128,128,.04);border-bottom:1px solid rgba(128,128,128,.08);border-radius:12px 12px 0 0}
.quote-preview-bar{width:3px;height:100%;min-height:28px;border-radius:2px;background:var(--tapp-primary,#6366f1);flex-shrink:0;margin-right:10px}
.quote-preview-body{flex:1;min-width:0}
.quote-preview-sender{font-size:11px;font-weight:600;color:var(--tapp-primary,#6366f1)}
.quote-preview-text{font-size:12px;color:var(--text-secondary,#888);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.quote-preview-close{background:none;border:none;font-size:18px;color:var(--text-secondary,#999);cursor:pointer;padding:2px 6px;border-radius:6px;flex-shrink:0;line-height:1}
.quote-preview-close:hover{background:rgba(128,128,128,.1)}

/* -- Forward Overlay -- */
.forward-overlay{position:fixed;inset:0;background:rgba(0,0,0,.4);display:flex;align-items:center;justify-content:center;z-index:200;animation:ctxFadeIn .15s ease}
.forward-sheet{background:var(--bg-primary,#fff);border-radius:16px;width:min(340px,90vw);max-height:60vh;display:flex;flex-direction:column;overflow:hidden;box-shadow:0 16px 48px rgba(0,0,0,.15)}
.forward-header{display:flex;align-items:center;justify-content:space-between;padding:14px 16px;border-bottom:1px solid rgba(128,128,128,.08)}
.forward-title{font-size:15px;font-weight:600;color:var(--text-primary,#1a1a1a)}
.forward-close{background:none;border:none;font-size:18px;color:var(--text-secondary,#999);cursor:pointer;padding:2px 6px;border-radius:6px}
.forward-close:hover{background:rgba(128,128,128,.1)}
.forward-list{overflow-y:auto;padding:6px}
.forward-item{display:flex;align-items:center;gap:10px;width:100%;padding:10px 12px;border:none;background:none;border-radius:10px;font-size:13px;font-weight:500;color:var(--text-primary,#1a1a1a);cursor:pointer;transition:background .12s;font-family:inherit}
.forward-item:hover{background:rgba(128,128,128,.06)}
.forward-item:active{background:rgba(128,128,128,.12)}
.forward-item-avatar{width:32px;height:32px;border-radius:50%;background:rgba(var(--tapp-primary-rgb,128,128,128),.1);color:var(--tapp-primary,#888);display:flex;align-items:center;justify-content:center;font-size:13px;font-weight:600;flex-shrink:0;overflow:hidden}
.forward-item-avatar img{width:100%;height:100%;object-fit:cover}
.dark .forward-sheet{background:var(--bg-primary,#1a1a1a)}
.dark .forward-header{border-color:rgba(255,255,255,.06)}
.dark .forward-title{color:rgba(255,255,255,.9)}
.dark .forward-item{color:rgba(255,255,255,.9)}

.msg-image{max-width:260px;max-height:200px;border-radius:10px;cursor:pointer;display:block}
.msg-file-card{display:flex;align-items:center;gap:10px;padding:10px 12px;border-radius:10px;background:rgba(128,128,128,.06);min-width:200px}
.msg-file-icon{width:36px;height:36px;border-radius:8px;display:flex;align-items:center;justify-content:center;font-size:17px;flex-shrink:0;background:rgba(128,128,128,.08)}
.msg-file-info{flex:1;min-width:0}
.msg-file-name{font-size:13px;font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.msg-file-size{font-size:11px;opacity:.55;margin-top:1px}
.msg-share-card{display:flex;gap:12px;padding:14px;border-radius:14px;background:rgba(128,128,128,.06);min-width:240px;max-width:320px;cursor:pointer;transition:background .15s}
.msg-share-card:active{background:rgba(128,128,128,.12)}
.msg-share-icon{width:46px;height:46px;border-radius:12px;display:flex;align-items:center;justify-content:center;font-size:20px;flex-shrink:0}
.msg-share-icon svg{width:24px;height:24px}
.msg-share-body{flex:1;min-width:0;display:flex;flex-direction:column;gap:3px}
.msg-share-type{font-size:10px;font-weight:600;text-transform:uppercase;letter-spacing:.04em;opacity:.5}
.msg-share-title{font-size:14px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.msg-share-desc{font-size:12px;opacity:.6;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;margin-top:1px}
.msg-share-meta{display:flex;align-items:center;gap:6px;margin-top:4px}
.msg-share-ver{font-size:10px;padding:2px 6px;border-radius:5px;background:rgba(128,128,128,.1);font-weight:500;letter-spacing:.02em}
.msg-share-status{font-size:10px;font-weight:600;padding:2px 8px;border-radius:5px}
.msg-share-status-pending{background:rgba(245,158,11,.15);color:#f59e0b}
.msg-share-status-accepted{background:rgba(34,197,94,.15);color:#22c55e}
.msg-share-status-rejected{background:rgba(239,68,68,.15);color:#ef4444}
.msg-share-actions{display:flex;gap:8px;margin-top:8px}
.msg-share-actions button{flex:1;padding:8px 0;border:none;border-radius:8px;font-size:12px;font-weight:600;cursor:pointer;transition:opacity .15s}
.msg-share-actions button:active{opacity:.7}
.msg-share-btn-accept{background:var(--tapp-primary,#6366f1);color:#fff}
.msg-share-btn-reject{background:rgba(128,128,128,.1);color:var(--text-secondary,#888)}
.bubble-local .msg-file-card,.bubble-local .msg-share-card{background:rgba(255,255,255,.15)}
.bubble-local .msg-file-icon,.bubble-local .msg-share-icon{background:rgba(255,255,255,.15)}
.bubble-local .msg-share-ver{background:rgba(255,255,255,.2)}
.bubble-remote .msg-share-icon{background:rgba(128,128,128,.08)}

/* ===== Members ===== */
.member-item{display:flex;align-items:center;gap:8px;padding:6px 8px;border-radius:10px;transition:background .15s}
.member-item:hover{background:rgba(128,128,128,.05)}
.member-avatar{width:24px;height:24px;border-radius:50%;background:rgba(var(--tapp-primary-rgb,128,128,128),.1);color:var(--tapp-primary,#888);display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;flex-shrink:0;overflow:hidden}
.member-avatar img{width:100%;height:100%;object-fit:cover}
.member-info{min-width:0;flex:1}
.member-name{font-size:12px;color:var(--text-primary,#1a1a1a);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.member-role{font-size:10px;color:var(--text-secondary,#999)}
.member-local{font-size:10px;color:var(--tapp-primary,#888);opacity:.7;flex-shrink:0}

/* ===== Dark Mode ===== */
.dark .aro-nav-item:hover{color:rgba(255,255,255,.9)}
.dark .sidebar{border-color:rgba(255,255,255,.06)}
.dark .sidebar-header{border-color:rgba(255,255,255,.06)}
.dark .sidebar-title{color:rgba(255,255,255,.92)}
.dark .conv-name{color:rgba(255,255,255,.9)}
.dark .conv-subtitle{color:rgba(255,255,255,.45)}
.dark .conv-empty{color:rgba(255,255,255,.45)}
.dark .chat-header{border-color:rgba(255,255,255,.06)}
.dark .chat-name{color:rgba(255,255,255,.92)}
.dark .back-btn{color:rgba(255,255,255,.6)}
.dark .member-panel{border-color:rgba(255,255,255,.06)}
.dark .member-header{border-color:rgba(255,255,255,.06)}
.dark .member-name{color:rgba(255,255,255,.9)}
.dark .member-title{color:rgba(255,255,255,.45)}
.dark .member-back-btn{color:rgba(255,255,255,.9)}
.dark .member-toggle-btn{color:rgba(255,255,255,.6)}
.dark .manage-btn{color:rgba(255,255,255,.6)}
.dark .empty-icon{background:rgba(255,255,255,.06);color:rgba(255,255,255,.35)}
.dark .empty-state{color:rgba(255,255,255,.5)}
.dark .empty-text{color:rgba(255,255,255,.5)}
.dark .bubble-remote{background:rgba(255,255,255,.07);color:rgba(255,255,255,.9)}
.dark .msg-input{color:rgba(255,255,255,.9)}
.dark .msg-time{opacity:.4}
.dark .msg-sender{color:var(--tapp-primary,#818cf8)}
.dark .messages-empty{color:rgba(255,255,255,.45)}
.dark .messages-empty-icon{background:rgba(255,255,255,.06);color:rgba(255,255,255,.35)}
.dark .pinned-bar{background:rgba(var(--tapp-primary-rgb,99,102,241),.1)}
.dark .pinned-bar-text{color:rgba(255,255,255,.85)}
.dark .manage-item{color:rgba(255,255,255,.9)}
.dark .meta-badge{color:rgba(255,255,255,.6)}
.dark .feed-handle{color:rgba(255,255,255,.45)}
.dark .feed-sidebar-stat{color:rgba(255,255,255,.45)}
.dark .feed-mobile-tab{color:rgba(255,255,255,.5)}
.dark .feed-item{border-color:rgba(255,255,255,.04)}
.dark .feed-item-handle{color:rgba(255,255,255,.45)}
.dark .feed-item-time{color:rgba(255,255,255,.4)}
.dark .feed-item-action{color:rgba(255,255,255,.45)}
.dark .feed-empty{color:rgba(255,255,255,.45)}
.dark .feed-follow-bar{border-color:rgba(255,255,255,.06)}
.dark .attach-preview{background:var(--bg-primary,#1a1a1a);border-color:rgba(255,255,255,.1)}
.dark .attach-preview-name{color:rgba(255,255,255,.9)}
.dark .picker-header-title{color:rgba(255,255,255,.92)}
.dark .picker-tab{background:rgba(255,255,255,.06);color:rgba(255,255,255,.5)}
.dark .picker-form label{color:rgba(255,255,255,.5)}
.dark .create-dialog-title{color:rgba(255,255,255,.92)}
.dark .create-dialog-tabs{background:rgba(255,255,255,.04)}
.dark .edit-label{color:rgba(255,255,255,.45)}
.dark .quote-preview{background:rgba(255,255,255,.04);border-color:rgba(255,255,255,.06)}
.dark .quote-preview-text{color:rgba(255,255,255,.5)}
.dark .invite-bar{border-color:rgba(255,255,255,.06)}
.dark .invite-input{background:rgba(255,255,255,.04);border-color:rgba(255,255,255,.1);color:rgba(255,255,255,.9)}
.dark .panel-detail-header{border-color:rgba(255,255,255,.06)}
.dark .ring-hdr-icon{background:rgba(var(--tapp-primary-rgb,128,128,128),.15);color:var(--tapp-primary,#818cf8)}
.dark .ring-action-sync{background:rgba(var(--tapp-primary-rgb,100,100,255),.15)}
.dark .ring-sync-bar{border-color:rgba(255,255,255,.04)}
.dark .ring-sync-bar.ring-sync-ok{background:rgba(34,197,94,.08)}
.dark .ring-sync-bar.ring-sync-err{background:rgba(239,68,68,.08)}
.dark .conv-avatar{color:var(--tapp-primary,#818cf8)}
.dark .member-avatar{color:var(--tapp-primary,#818cf8)}
.dark .msg-file-card{background:rgba(255,255,255,.08)}
.dark .msg-file-icon{background:rgba(255,255,255,.1)}
.dark .msg-share-card{background:rgba(255,255,255,.08)}

/* ===== Responsive ===== */
@media(max-width:768px){
  .aro-nav{order:99;border-bottom:none;border-top:1px solid rgba(128,128,128,.08);padding:6px 6px calc(6px + env(safe-area-inset-bottom,0px));justify-content:space-around}
  .aro-nav-item{flex-direction:column;gap:3px;padding:8px 12px;font-size:11px}
  .aro-nav-item svg{width:22px;height:22px}
  .nav-feed-avatar{width:24px;height:24px;font-size:11px}
  .sidebar{width:100%}
  .sidebar-hidden-mobile{display:none!important}
  .back-btn{display:block}
  .member-panel{display:none;position:fixed;inset:0;width:100%!important;z-index:80;background:var(--bg-primary,#fff);border-left:none}
  .dark .member-panel{background:var(--bg-primary,#1a1a1a)}
  .member-panel.member-open-mobile{display:flex!important}
  .member-back-btn{display:flex}
  .aro-panel-layout .sidebar{width:100%}
  .aro-panel-layout .sidebar-hidden-mobile{display:none!important}
  .aro-panel-layout .panel-main{display:none}
  .aro-panel-layout .panel-main-show-mobile{display:flex!important}
}
@media(min-width:769px) and (max-width:1024px){
  .member-panel{width:0;border-left:none;overflow:hidden}
  .member-panel.member-expanded-tablet{width:180px;border-left:1px solid rgba(128,128,128,.08);overflow:hidden}
  .feed-sidebar{width:68px;padding:12px 8px;overflow:visible;position:relative;z-index:5}
  .feed-nav-item span{display:none}
  .feed-nav-item{justify-content:center;padding:10px}
  .feed-sidebar-footer{align-items:center;position:relative}
  .feed-profile-card{width:40px;height:40px;padding:0;border:none;background:none;overflow:visible}
  .feed-profile-summary{width:40px;height:40px;justify-content:center}
  .feed-profile-card>.feed-profile-summary .feed-profile-info,.feed-profile-card>.feed-profile-summary .feed-profile-copy,.feed-profile-card>.feed-profile-summary .feed-profile-toggle,.feed-profile-card>.feed-profile-details{display:none!important}
  .feed-profile-card.feed-profile-popover-open .feed-profile-tablet-popover{position:absolute;left:54px;bottom:0;width:232px;padding:9px;border:1px solid rgba(128,128,128,.12);border-radius:12px;background:var(--bg-primary,#fff);box-shadow:0 12px 36px rgba(0,0,0,.18);z-index:40;display:flex;flex-direction:column}
  .feed-profile-card.feed-profile-popover-open .feed-profile-tablet-popover .feed-profile-info{display:flex!important}
  .feed-profile-card.feed-profile-popover-open .feed-profile-tablet-popover .feed-profile-copy,.feed-profile-card.feed-profile-popover-open .feed-profile-tablet-popover .feed-profile-toggle{display:flex!important}
  .feed-profile-card.feed-profile-popover-open .feed-profile-tablet-popover .feed-profile-details{display:block!important}
  .feed-profile-card.feed-profile-popover-open.feed-profile-expanded .feed-profile-tablet-popover .feed-profile-details{max-height:56px;opacity:1;padding-top:8px}
  .dark .feed-profile-card.feed-profile-popover-open .feed-profile-tablet-popover{background:var(--bg-primary,#0a0a0a);border-color:rgba(255,255,255,.08);box-shadow:0 16px 44px rgba(0,0,0,.38)}
  .feed-sidebar-stats{display:none}
}

/* ===== Create Dialog ===== */
.create-overlay{position:fixed;inset:0;background:rgba(0,0,0,.4);display:flex;align-items:center;justify-content:center;z-index:100}
.create-dialog{background:var(--bg-primary,#fff);border-radius:16px;width:min(360px,90vw);padding:20px;box-shadow:0 16px 48px rgba(0,0,0,.15)}
.create-dialog-header{display:flex;align-items:center;justify-content:space-between;margin-bottom:16px}
.create-dialog-title{margin:0;font-size:16px;font-weight:600;color:var(--text-primary,#1a1a1a)}
.create-dialog-close{background:none;border:none;font-size:16px;color:var(--text-secondary,#999);cursor:pointer;padding:4px;border-radius:6px}
.create-dialog-close:hover{background:rgba(128,128,128,.08)}
.create-dialog-tabs{display:flex;gap:4px;margin-bottom:16px;background:rgba(128,128,128,.06);border-radius:10px;padding:3px}
.create-tab{flex:1;padding:6px 12px;border:none;background:none;border-radius:8px;font-size:12px;font-weight:500;color:var(--text-secondary,#999);cursor:pointer;transition:all .15s}
.create-tab-active{background:var(--bg-primary,#fff);color:var(--text-primary,#1a1a1a);box-shadow:0 1px 3px rgba(0,0,0,.08)}
.create-form{display:flex;flex-direction:column;gap:10px}
.create-input{height:40px;padding:0 14px;border-radius:12px;border:1px solid rgba(128,128,128,.15);background:rgba(128,128,128,.03);color:var(--text-primary,#1a1a1a);font-size:13px;outline:none;transition:border-color .2s}
.create-input:focus{border-color:rgba(var(--tapp-primary-rgb,128,128,128),.4)}
.create-input::placeholder{color:var(--text-secondary,#bbb)}
.create-submit{height:40px;border-radius:12px;border:none;background:var(--tapp-primary,#888);color:#fff;font-size:13px;font-weight:500;cursor:pointer;transition:opacity .15s}
.create-submit:hover{opacity:.85}
.create-submit:disabled{opacity:.4;cursor:not-allowed}
.dark .create-dialog{background:var(--bg-primary,#1a1a1a)}
.dark .create-tab-active{background:rgba(255,255,255,.08);color:rgba(255,255,255,.92)}
.dark .create-input{background:rgba(255,255,255,.03);border-color:rgba(255,255,255,.1);color:rgba(255,255,255,.9)}
.edit-label{font-size:11px;font-weight:600;color:var(--text-secondary,#888);text-transform:uppercase;letter-spacing:.04em}
.member-kick{margin-left:auto;width:20px;height:20px;border:none;background:none;color:var(--text-secondary,#999);cursor:pointer;border-radius:4px;display:flex;align-items:center;justify-content:center;opacity:0;transition:opacity .15s,background .15s}
.member-item:hover .member-kick{opacity:1}
.member-kick:hover{background:rgba(239,68,68,.1);color:#ef4444}

/* ===== In-app Confirm Dialog (native confirm() is blocked in the sandboxed iframe) ===== */
.confirm-overlay{position:fixed;inset:0;background:rgba(0,0,0,.4);display:flex;align-items:center;justify-content:center;z-index:300;animation:ctxFadeIn .12s ease}
.confirm-dialog{background:var(--bg-primary,#fff);border-radius:16px;width:min(320px,88vw);padding:20px;box-shadow:0 16px 48px rgba(0,0,0,.18)}
.confirm-message{font-size:14px;line-height:1.6;color:var(--text-primary,#1a1a1a);margin-bottom:18px;overflow-wrap:break-word}
.confirm-actions{display:flex;gap:8px;justify-content:flex-end}
.confirm-btn{padding:8px 18px;border:none;border-radius:10px;font-size:13px;font-weight:600;cursor:pointer;transition:opacity .15s;font-family:inherit}
.confirm-btn:hover{opacity:.85}
.confirm-btn-cancel{background:rgba(128,128,128,.08);color:var(--text-primary,#1a1a1a)}
.confirm-btn-ok{background:var(--tapp-primary,#6366f1);color:#fff}
.confirm-btn-danger{background:#ef4444}
.dark .confirm-dialog{background:var(--bg-primary,#1a1a1a)}
.dark .confirm-message{color:rgba(255,255,255,.9)}
.dark .confirm-btn-cancel{background:rgba(255,255,255,.08);color:rgba(255,255,255,.9)}
`

const ARO_I18N: Record<string, Record<string, string>> = {
  "en": {
    "accept": "Accept",
    "acceptConfirmDesc": "Someone shared a Tapp app with you",
    "acceptConfirmTitle": "Install this Tapp?",
    "acceptFail": "Accept failed",
    "acceptTapp": "Accept",
    "activityType": "Activity",
    "addPeerBtn": "Add",
    "addPeerFail": "Add failed",
    "addPeerPlaceholder": "Actor URL or @user@domain",
    "adminRequired": "Administrator access required",
    "alreadyLatest": "Already up to date",
    "attachBrew": "Brew",
    "attachBrewPrompt": "Enter Brew article title",
    "attachFile": "File",
    "attachImage": "Image",
    "attachLibrary": "Library",
    "attachLibraryPrompt": "Enter library name",
    "attachReport": "Report",
    "attachReportPrompt": "Enter report title",
    "attachSending": "Sending...",
    "attachTapp": "Tapp",
    "attachTappPrompt": "Enter Tapp ID or name",
    "channelPlaceholder": "Actor URL or @user@domain",
    "close": "Close",
    "closeChannelConfirm": "Close this channel? You won't be able to send messages afterwards.",
    "closed": "Closed",
    "collapseDetails": "Collapse",
    "composeAddImage": "Image",
    "composeAddVideo": "Video",
    "composeCancel": "Cancel",
    "composeEmpty": "Add text or attach an image/video",
    "composeFail": "Publish failed",
    "composePlaceholder": "Share something…",
    "composePost": "Post",
    "composePublish": "Publish",
    "composePublishing": "Publishing…",
    "composeSuccess": "Published",
    "composeUploading": "Uploading…",
    "confirmCancel": "Cancel",
    "confirmOk": "OK",
    "connected": "Connected",
    "copied": "Copied",
    "copy": "Copy",
    "copyFail": "Copy failed",
    "create": "New",
    "createChannel": "Create Channel",
    "createFail": "Create failed",
    "createRingBtn": "Create",
    "createRingFail": "Create failed",
    "createRingTitle": "Create Ring",
    "createRoom": "Create Room",
    "creating": "Creating...",
    "dateToday": "Today",
    "dateYesterday": "Yesterday",
    "disconnected": "Disconnected",
    "dissolve": "Dissolve Group",
    "dissolveConfirm": "Are you sure you want to dissolve this group? This cannot be undone.",
    "dissolveFail": "Dissolve failed",
    "dm": "DM",
    "editRoom": "Edit Room",
    "emptyChatHint": "Send the first message",
    "emptyFollowers": "No followers",
    "emptyFollowing": "Not following anyone",
    "emptyPeers": "No peers",
    "emptyPublished": "No published content",
    "emptyRings": "No rings",
    "emptyRoomHint": "Send the first message to start group chat",
    "emptyTimeline": "No activity",
    "expandDetails": "Expand",
    "feedFollowers": "Followers",
    "feedFollowing": "Following",
    "feedItems": "items",
    "feedLoadFail": "Failed to load feed",
    "feedPublished": "Published",
    "feedTimeline": "Feed",
    "fileTooLarge": "File too large (max 10MB)",
    "followBtn": "Follow",
    "followFail": "Follow failed",
    "followPlaceholder": "Actor URL or @user@domain",
    "followQueued": "Follow request sent; the remote instance auto-accepts (no manual approval)",
    "forwardSuccess": "Forwarded",
    "forwardTo": "Forward to...",
    "installBtn": "Install",
    "installFailed": "Install failed, tap to retry",
    "installSuccess": "✓ Installed",
    "installedAt": "Installed",
    "installingBtn": "Installing...",
    "invite": "Invite...",
    "inviteBtn": "Invite",
    "inviteFail": "Invite failed",
    "inviteFromContacts": "From contacts",
    "inviteManual": "Manual invite",
    "invitePlaceholder": "Actor URL or @user@domain",
    "inviteSuccess": "Invited",
    "invited": "Invited",
    "inviting": "Inviting...",
    "kick": "Remove",
    "kickConfirm": "Remove this member?",
    "kickFail": "Remove failed",
    "leave": "Leave",
    "leaveBtn": "Leave Ring",
    "leaveRingConfirm": "Leave this ring?",
    "leaveRingFail": "Leave failed",
    "loadFail": "Load failed",
    "local": "Local",
    "localVer": "Local",
    "manage": "Manage",
    "mediaTooLarge": "File too large",
    "mediaUnsupported": "Unsupported file type",
    "members": "Members",
    "msgActions": "Message actions",
    "msgForward": "Forward",
    "msgPin": "Pin",
    "msgQuote": "Quote",
    "msgUnpin": "Unpin",
    "navFeed": "Feed",
    "navMessages": "Messages",
    "navRings": "Rings",
    "newChannel": "New DM",
    "newMessage": "New message",
    "newRoom": "New Room",
    "noContacts": "No contacts to invite",
    "noConv": "No conversations",
    "noConvHint": "Tap + to start a conversation",
    "openOriginal": "Open original",
    "openTappBtn": "Open Tapp",
    "peers": "peers",
    "pending": "Pending",
    "pendingConfirm": "Awaiting confirmation",
    "pickerCancel": "Cancel",
    "pickerConfirm": "Confirm",
    "pickerDesc": "Description (optional)",
    "pickerEmpty": "No data",
    "pickerLoading": "Loading...",
    "pickerSearchPlaceholder": "Search...",
    "pickerSelectPlatform": "Select platform",
    "pickerTitle": "Title",
    "pinFail": "Pin failed",
    "pinnedMsg": "Pinned",
    "previewFile": "📎 File",
    "previewImage": "📷 Image",
    "previewSystem": "System message",
    "publicFeed": "Public feed",
    "quoteLabel": "Quote",
    "refresh": "Refresh",
    "rejectTapp": "Decline",
    "remoteVer": "Shared",
    "removeBtn": "Unpublish",
    "removePeerFail": "Remove failed",
    "ringNamePlaceholder": "Ring name",
    "ringPeersTitle": "Peers",
    "ringType": "Type",
    "ringTypeBrewRecommend": "Brew Picks",
    "ringTypeInstanceDirectory": "Instance Directory",
    "ringTypeLibraryExchange": "Library Exchange",
    "ringTypeTappStore": "Tapp Store",
    "roleAdmin": "Admin",
    "roleMember": "Member",
    "roleOwner": "Owner",
    "roomDesc": "Description",
    "roomName": "Room name",
    "roomPlaceholder": "Room name",
    "save": "Save",
    "saveFail": "Save failed",
    "saving": "Saving...",
    "selectBrew": "Select Brew article",
    "selectHint": "Select a conversation to start chatting",
    "selectLibrary": "Select from library",
    "selectReport": "Select report",
    "selectRing": "Select a ring to view details",
    "selectTapp": "Select Tapp",
    "send": "Send",
    "sendFail": "Send failed",
    "syncBtn": "Sync",
    "syncFail": "Sync failed",
    "syncSuccess": "Sync complete",
    "syncing": "Syncing...",
    "tappInstalled": "Installed",
    "tappNotInstalled": "Not installed",
    "tappReceived": "Tapp shared",
    "tappShareAccepted": "Accepted",
    "tappSharePending": "Waiting for acceptance",
    "tappShareRejected": "Declined",
    "tappUpdateAvail": "Update available",
    "title": "Messenger",
    "typing": "Type a message...",
    "unfollowBtn": "Unfollow",
    "unfollowFail": "Unfollow failed",
    "unpublishFail": "Unpublish failed",
    "updatingBtn": "Update",
  },
  "ja": {
    "accept": "承認",
    "acceptConfirmDesc": "Tappアプリが共有されました",
    "acceptConfirmTitle": "このTappをインストール？",
    "acceptFail": "承認失敗",
    "acceptTapp": "承認",
    "activityType": "アクティビティ",
    "addPeerBtn": "追加",
    "addPeerFail": "追加失敗",
    "addPeerPlaceholder": "Actor URL または @user@domain",
    "adminRequired": "管理者権限が必要です",
    "alreadyLatest": "最新版です",
    "attachBrew": "Brew",
    "attachBrewPrompt": "Brew記事タイトルを入力",
    "attachFile": "ファイル",
    "attachImage": "画像",
    "attachLibrary": "ライブラリ",
    "attachLibraryPrompt": "ライブラリ名を入力",
    "attachReport": "レポート",
    "attachReportPrompt": "レポートタイトルを入力",
    "attachSending": "送信中...",
    "attachTapp": "Tapp",
    "attachTappPrompt": "Tapp IDまたは名前を入力",
    "channelPlaceholder": "Actor URL または @user@domain",
    "close": "閉じる",
    "closeChannelConfirm": "このチャネルを閉じますか？閉じると送信できなくなります。",
    "closed": "クローズ",
    "collapseDetails": "閉じる",
    "composeAddImage": "画像",
    "composeAddVideo": "動画",
    "composeCancel": "キャンセル",
    "composeEmpty": "テキストか画像/動画を追加してください",
    "composeFail": "公開に失敗しました",
    "composePlaceholder": "いまどうしてる？",
    "composePost": "投稿",
    "composePublish": "公開",
    "composePublishing": "公開中…",
    "composeSuccess": "公開しました",
    "composeUploading": "アップロード中…",
    "confirmCancel": "キャンセル",
    "confirmOk": "OK",
    "connected": "接続済み",
    "copied": "コピーしました",
    "copy": "コピー",
    "copyFail": "コピー失敗",
    "create": "新規",
    "createChannel": "チャンネル作成",
    "createFail": "作成失敗",
    "createRingBtn": "作成",
    "createRingFail": "作成失敗",
    "createRingTitle": "リング作成",
    "createRoom": "ルーム作成",
    "creating": "作成中...",
    "dateToday": "今日",
    "dateYesterday": "昨日",
    "disconnected": "切断",
    "dissolve": "グループ解散",
    "dissolveConfirm": "このグループを解散しますか？この操作は元に戻せません。",
    "dissolveFail": "解散失敗",
    "dm": "DM",
    "editRoom": "ルーム編集",
    "emptyChatHint": "最初のメッセージを送信してください",
    "emptyFollowers": "フォロワーなし",
    "emptyFollowing": "フォローなし",
    "emptyPeers": "ピアなし",
    "emptyPublished": "公開コンテンツなし",
    "emptyRings": "リングなし",
    "emptyRoomHint": "最初のメッセージを送信してグループチャットを始めましょう",
    "emptyTimeline": "アクティビティなし",
    "expandDetails": "展開",
    "feedFollowers": "フォロワー",
    "feedFollowing": "フォロー中",
    "feedItems": "件",
    "feedLoadFail": "フィードの読み込みに失敗",
    "feedPublished": "公開済み",
    "feedTimeline": "フィード",
    "fileTooLarge": "ファイルが大きすぎます（最大10MB）",
    "followBtn": "フォロー",
    "followFail": "フォロー失敗",
    "followPlaceholder": "Actor URL または @user@domain",
    "followQueued": "フォローリクエストを送信しました。相手側は自動承認します（手動承認は不要）",
    "forwardSuccess": "転送済み",
    "forwardTo": "転送先...",
    "installBtn": "インストール",
    "installFailed": "インストール失敗、タップで再試行",
    "installSuccess": "✓ インストール完了",
    "installedAt": "インストール日",
    "installingBtn": "インストール中...",
    "invite": "招待...",
    "inviteBtn": "招待",
    "inviteFail": "招待失敗",
    "inviteFromContacts": "連絡先から選択",
    "inviteManual": "手動招待",
    "invitePlaceholder": "Actor URL または @user@domain",
    "inviteSuccess": "招待済み",
    "invited": "招待済み",
    "inviting": "招待中...",
    "kick": "除外",
    "kickConfirm": "このメンバーを削除しますか？",
    "kickFail": "除外失敗",
    "leave": "退出",
    "leaveBtn": "リング退出",
    "leaveRingConfirm": "このリングから退出しますか？",
    "leaveRingFail": "退出失敗",
    "loadFail": "読み込み失敗",
    "local": "ローカル",
    "localVer": "ローカル",
    "manage": "管理",
    "mediaTooLarge": "ファイルが大きすぎます",
    "mediaUnsupported": "未対応のファイル形式です",
    "members": "メンバー",
    "msgActions": "メッセージ操作",
    "msgForward": "転送",
    "msgPin": "ピン留め",
    "msgQuote": "引用",
    "msgUnpin": "ピン解除",
    "navFeed": "フィード",
    "navMessages": "メッセージ",
    "navRings": "リング",
    "newChannel": "新規DM",
    "newMessage": "新しいメッセージ",
    "newRoom": "新規ルーム",
    "noContacts": "招待可能な連絡先なし",
    "noConv": "会話なし",
    "noConvHint": "右上の + から会話を開始",
    "openOriginal": "元記事を開く",
    "openTappBtn": "Tappを開く",
    "peers": "ピア",
    "pending": "保留中",
    "pendingConfirm": "確認待ち",
    "pickerCancel": "キャンセル",
    "pickerConfirm": "確認",
    "pickerDesc": "説明（任意）",
    "pickerEmpty": "データなし",
    "pickerLoading": "読み込み中...",
    "pickerSearchPlaceholder": "検索...",
    "pickerSelectPlatform": "プラットフォーム選択",
    "pickerTitle": "タイトル",
    "pinFail": "ピン留めに失敗",
    "pinnedMsg": "ピン留め",
    "previewFile": "📎 ファイル",
    "previewImage": "📷 画像",
    "previewSystem": "システムメッセージ",
    "publicFeed": "公開フィード",
    "quoteLabel": "引用",
    "refresh": "リフレッシュ",
    "rejectTapp": "拒否",
    "remoteVer": "共有",
    "removeBtn": "公開取消",
    "removePeerFail": "削除失敗",
    "ringNamePlaceholder": "リング名",
    "ringPeersTitle": "ピア",
    "ringType": "タイプ",
    "ringTypeBrewRecommend": "Brew おすすめ",
    "ringTypeInstanceDirectory": "インスタンス一覧",
    "ringTypeLibraryExchange": "資料交換",
    "ringTypeTappStore": "Tapp ストア",
    "roleAdmin": "管理者",
    "roleMember": "メンバー",
    "roleOwner": "オーナー",
    "roomDesc": "説明",
    "roomName": "ルーム名",
    "roomPlaceholder": "ルーム名",
    "save": "保存",
    "saveFail": "保存失敗",
    "saving": "保存中...",
    "selectBrew": "Brew記事を選択",
    "selectHint": "会話を選んでチャットを始めましょう",
    "selectLibrary": "ライブラリから選択",
    "selectReport": "レポートを選択",
    "selectRing": "リングを選択して詳細を表示",
    "selectTapp": "Tappを選択",
    "send": "送信",
    "sendFail": "送信失敗",
    "syncBtn": "同期",
    "syncFail": "同期失敗",
    "syncSuccess": "同期完了",
    "syncing": "同期中...",
    "tappInstalled": "インストール済み",
    "tappNotInstalled": "未インストール",
    "tappReceived": "Tappが共有されました",
    "tappShareAccepted": "承認済み",
    "tappSharePending": "承認待ち",
    "tappShareRejected": "拒否済み",
    "tappUpdateAvail": "更新あり",
    "title": "メッセンジャー",
    "typing": "メッセージを入力...",
    "unfollowBtn": "フォロー解除",
    "unfollowFail": "フォロー解除失敗",
    "unpublishFail": "公開取消失敗",
    "updatingBtn": "更新",
  },
  "zh": {
    "accept": "接受",
    "acceptConfirmDesc": "对方向你分享了一个 Tapp 应用",
    "acceptConfirmTitle": "安装此 Tapp？",
    "acceptFail": "接受失败",
    "acceptTapp": "接受",
    "activityType": "活动类型",
    "addPeerBtn": "添加",
    "addPeerFail": "添加失败",
    "addPeerPlaceholder": "Actor URL 或 @user@domain",
    "adminRequired": "需要管理员权限",
    "alreadyLatest": "已是最新版本",
    "attachBrew": "Brew",
    "attachBrewPrompt": "输入 Brew 文章标题",
    "attachFile": "文件",
    "attachImage": "图片",
    "attachLibrary": "资料库",
    "attachLibraryPrompt": "输入资料库名称",
    "attachReport": "报告",
    "attachReportPrompt": "输入报告标题",
    "attachSending": "发送中...",
    "attachTapp": "Tapp",
    "attachTappPrompt": "输入 Tapp ID 或名称",
    "channelPlaceholder": "Actor URL 或 @user@domain",
    "close": "关闭",
    "closeChannelConfirm": "确定关闭此私信通道？关闭后将无法继续发送消息。",
    "closed": "已关闭",
    "collapseDetails": "收起",
    "composeAddImage": "图片",
    "composeAddVideo": "视频",
    "composeCancel": "取消",
    "composeEmpty": "请输入文字或添加图片/视频",
    "composeFail": "发布失败",
    "composePlaceholder": "分享点什么…",
    "composePost": "发帖",
    "composePublish": "发布",
    "composePublishing": "发布中…",
    "composeSuccess": "已发布",
    "composeUploading": "上传中…",
    "confirmCancel": "取消",
    "confirmOk": "确定",
    "connected": "已连接",
    "copied": "已复制",
    "copy": "复制",
    "copyFail": "复制失败",
    "create": "新建",
    "createChannel": "创建通道",
    "createFail": "创建失败",
    "createRingBtn": "创建",
    "createRingFail": "创建失败",
    "createRingTitle": "创建环网",
    "createRoom": "创建房间",
    "creating": "创建中...",
    "dateToday": "今天",
    "dateYesterday": "昨天",
    "disconnected": "未连接",
    "dissolve": "解散群组",
    "dissolveConfirm": "确定要解散此群组吗？此操作不可撤销。",
    "dissolveFail": "解散失败",
    "dm": "私信",
    "editRoom": "编辑房间",
    "emptyChatHint": "发送第一条消息开始聊天",
    "emptyFollowers": "暂无粉丝",
    "emptyFollowing": "暂无关注",
    "emptyPeers": "暂无节点",
    "emptyPublished": "暂无发布内容",
    "emptyRings": "暂无环网",
    "emptyRoomHint": "发送第一条消息开始群聊",
    "emptyTimeline": "暂无动态",
    "expandDetails": "展开",
    "feedFollowers": "粉丝",
    "feedFollowing": "关注",
    "feedItems": "项",
    "feedLoadFail": "动态加载失败",
    "feedPublished": "已发布",
    "feedTimeline": "动态",
    "fileTooLarge": "文件过大（最大 10MB）",
    "followBtn": "关注",
    "followFail": "关注失败",
    "followPlaceholder": "Actor URL 或 @user@domain",
    "followQueued": "关注请求已发送；对方实例将自动接受（无需手动批准）",
    "forwardSuccess": "已转发",
    "forwardTo": "转发到...",
    "installBtn": "安装",
    "installFailed": "安装失败，点击重试",
    "installSuccess": "✓ 安装成功",
    "installedAt": "安装时间",
    "installingBtn": "安装中...",
    "invite": "邀请...",
    "inviteBtn": "邀请",
    "inviteFail": "邀请失败",
    "inviteFromContacts": "从联系人中选择",
    "inviteManual": "手动邀请",
    "invitePlaceholder": "Actor URL 或 @user@domain",
    "inviteSuccess": "已邀请",
    "invited": "已邀请",
    "inviting": "邀请中...",
    "kick": "移除",
    "kickConfirm": "确定移除该成员？",
    "kickFail": "移除失败",
    "leave": "离开",
    "leaveBtn": "退出环网",
    "leaveRingConfirm": "确定退出此环网？",
    "leaveRingFail": "退出失败",
    "loadFail": "加载失败",
    "local": "本地",
    "localVer": "本地版本",
    "manage": "管理",
    "mediaTooLarge": "文件过大",
    "mediaUnsupported": "不支持的文件类型",
    "members": "成员",
    "msgActions": "消息操作",
    "msgForward": "转发",
    "msgPin": "置顶",
    "msgQuote": "引用",
    "msgUnpin": "取消置顶",
    "navFeed": "动态",
    "navMessages": "信使",
    "navRings": "环网",
    "newChannel": "新建私信",
    "newMessage": "新消息",
    "newRoom": "新建群聊",
    "noContacts": "暂无可邀请的联系人",
    "noConv": "暂无会话",
    "noConvHint": "点击右上角 + 开始新会话",
    "openOriginal": "查看原文",
    "openTappBtn": "打开 Tapp",
    "peers": "节点",
    "pending": "待接受",
    "pendingConfirm": "等待确认",
    "pickerCancel": "取消",
    "pickerConfirm": "确认",
    "pickerDesc": "描述（可选）",
    "pickerEmpty": "暂无数据",
    "pickerLoading": "加载中...",
    "pickerSearchPlaceholder": "搜索...",
    "pickerSelectPlatform": "选择平台",
    "pickerTitle": "标题",
    "pinFail": "置顶失败",
    "pinnedMsg": "置顶消息",
    "previewFile": "📎 文件",
    "previewImage": "📷 图片",
    "previewSystem": "系统消息",
    "publicFeed": "公开动态",
    "quoteLabel": "引用",
    "refresh": "刷新",
    "rejectTapp": "拒绝",
    "remoteVer": "分享版本",
    "removeBtn": "取消发布",
    "removePeerFail": "移除失败",
    "ringNamePlaceholder": "环网名称",
    "ringPeersTitle": "节点",
    "ringType": "类型",
    "ringTypeBrewRecommend": "Brew 推荐",
    "ringTypeInstanceDirectory": "实例目录",
    "ringTypeLibraryExchange": "资料交换",
    "ringTypeTappStore": "Tapp 商店",
    "roleAdmin": "管理员",
    "roleMember": "成员",
    "roleOwner": "群主",
    "roomDesc": "房间描述",
    "roomName": "房间名称",
    "roomPlaceholder": "房间名称",
    "save": "保存",
    "saveFail": "保存失败",
    "saving": "保存中...",
    "selectBrew": "选择 Brew 文章",
    "selectHint": "选择一个会话开始聊天",
    "selectLibrary": "选择资料",
    "selectReport": "选择报告",
    "selectRing": "选择一个环网查看详情",
    "selectTapp": "选择 Tapp",
    "send": "发送",
    "sendFail": "发送失败",
    "syncBtn": "同步",
    "syncFail": "同步失败",
    "syncSuccess": "同步完成",
    "syncing": "同步中...",
    "tappInstalled": "已安装",
    "tappNotInstalled": "未安装",
    "tappReceived": "收到 Tapp 分享",
    "tappShareAccepted": "已接受",
    "tappSharePending": "等待对方接受",
    "tappShareRejected": "已拒绝",
    "tappUpdateAvail": "有更新可用",
    "title": "信使",
    "typing": "输入消息...",
    "unfollowBtn": "取消关注",
    "unfollowFail": "取消关注失败",
    "unpublishFail": "取消发布失败",
    "updatingBtn": "更新",
  }
}

// ==================== Page Modules ====================
const PAGE_MOD_I18N = `\
// i18n — loads from sandbox-injected window._TAPP_I18N
var LANG = window._TAPP_I18N || {};
var lang = LANG.zh || {};
var currentLocale = 'zh';

function setLocale(locale) {
  currentLocale = locale || 'zh';
  var key = currentLocale.startsWith('zh') ? 'zh' : currentLocale.startsWith('ja') ? 'ja' : 'en';
  lang = LANG[key] || LANG.en || {};
}
`

const PAGE_MOD_STATE = `\
// ==================== State ====================
var state = {
  channels: [],
  rooms: [],
  activeKind: null,
  activeId: null,
  messages: [],
  messagesFp: '',
  members: [],
  channelDetail: null,
  roomDetail: null,
  sending: false,
  pollTimer: null,
  pollInterval: 15000,
  /** 新消息应用内 Toast（设置项 notifyOnMessage） */
  notifyOnMessage: true,
  /** Active realtime WS subscription (channel|room) */
  subscribedKind: null,
  subscribedId: null,
  realtimeBound: false,
  localActorUrl: null,
  identity: null,
  userRole: 'guest',
  isGuest: true,
  isAdmin: false,
  // Attachment
  pendingAttach: null, // { type: 'image'|'file'|'tapp'|'brew'|'library'|'report', data, name, size, mime }
  // Aro views
  currentView: 'feed',
  // Feed (merged timeline + profile)
  feedSubTab: 'timeline',
  feedLoading: false,
  feedError: null,
  feedLoaded: {
    timeline: false,
    following: false,
    followers: false,
    published: false,
  },
  timeline: [],
  following: [],
  followers: [],
  published: [],
  // Rings
  rings: [],
  // Ring detail
  activeRingId: null,
  ringDetail: null,
  ringPeers: [],
  // Tapp accept/reject state map
  tappAcceptMap: {},
  // Quote reply
  quoteMsg: null,
};

var $ = function (id) { return document.getElementById(id); };

// SVG icon constants (replacing emoji for consistency)
var SVG_ICONS = {
  tapp: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 16V8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16z"/><path d="M3.27 6.96L12 12.01l8.73-5.05M12 22.08V12"/></svg>',
  brew: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 8h1a4 4 0 010 8h-1"/><path d="M3 8h14v9a4 4 0 01-4 4H7a4 4 0 01-4-4V8z"/><path d="M6 2v3M10 2v3M14 2v3"/></svg>',
  library: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19.5A2.5 2.5 0 016.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/></svg>',
  report: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 20V10M12 20V4M6 20v-6"/></svg>',
  file: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48"/></svg>',
  channel: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>',
  room: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 00-3-3.87M16 3.13a4 4 0 010 7.75"/></svg>',
  memo: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>',
  page: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6M16 13H8M16 17H8M10 9H8"/></svg>',
  coffee: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8z"/><path d="M6 1v3M10 1v3M14 1v3"/></svg>',
  puzzle: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 01-.837.276c-.47-.07-.802-.48-.968-.925a2.501 2.501 0 10-3.214 3.214c.446.166.855.497.925.968a.979.979 0 01-.276.837l-1.61 1.61a2.404 2.404 0 01-1.705.707 2.402 2.402 0 01-1.704-.706l-1.568-1.568a1.026 1.026 0 00-.877-.29c-.493.074-.84.504-1.02.968a2.5 2.5 0 11-3.237-3.237c.464-.18.894-.527.967-1.02a1.026 1.026 0 00-.289-.877l-1.568-1.568A2.41 2.41 0 011.998 12c0-.617.236-1.234.706-1.704L4.315 8.685a.98.98 0 01.837-.276c.47.07.802.48.968.925a2.501 2.501 0 103.214-3.214c-.446-.166-.855-.497-.925-.968a.979.979 0 01.276-.837l1.61-1.61a2.404 2.404 0 011.705-.707c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.074.84-.504 1.02-.968a2.5 2.5 0 113.237 3.237c-.464.18-.894.527-.967 1.02z"/></svg>',
  globe: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"/></svg>',
  ring: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M0 15L24 9"/></svg>',
  star: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" stroke="currentColor" stroke-width="1"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87L18.18 22 12 18.56 5.82 22 7 14.14l-5-4.87 6.91-1.01L12 2z"/></svg>',
  mail: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="16" rx="2"/><path d="M22 4L12 13 2 4"/></svg>',
  antenna: '<svg viewBox="0 0 24 24" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4.9 19.1l2.8-2.8M7 4l3.5 3.5M16.5 20l-3.5-3.5M2 12h3M19 12h3M12 2v3M12 19v3"/><circle cx="12" cy="12" r="4"/></svg>',
};
`

const PAGE_MOD_HELPERS = `\
// ==================== Helpers ====================
function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
function timeStr(iso) { try { return new Date(iso).toLocaleTimeString(currentLocale, { hour: '2-digit', minute: '2-digit' }); } catch (e) { return ''; } }
function fullTimeStr(iso) { try { return new Date(iso).toLocaleString(currentLocale); } catch (e) { return ''; } }
/** Relative short time for conv list (e.g. 5m, 2h, 3d). */
function relTimeStr(iso) {
  if (!iso) return '';
  try {
    if (typeof timeAgo === 'function') return timeAgo(iso);
  } catch (e) { /* fall through */ }
  try {
    var d = new Date(iso);
    var sec = Math.floor((Date.now() - d) / 1000);
    if (sec < 60) return sec + 's';
    var min = Math.floor(sec / 60);
    if (min < 60) return min + 'm';
    var hr = Math.floor(min / 60);
    if (hr < 24) return hr + 'h';
    var day = Math.floor(hr / 24);
    if (day < 30) return day + 'd';
    return d.toLocaleDateString(currentLocale, { month: 'short', day: 'numeric' });
  } catch (e2) { return ''; }
}

/** 消息日期分隔线标签：今天/昨天/日期 */
function dayLabel(iso) {
  var d = new Date(iso);
  if (isNaN(d)) return '';
  var now = new Date();
  var startOfDay = function (x) { return new Date(x.getFullYear(), x.getMonth(), x.getDate()); };
  var diffDays = Math.round((startOfDay(now) - startOfDay(d)) / 86400000);
  if (diffDays === 0) return lang.dateToday || 'Today';
  if (diffDays === 1) return lang.dateYesterday || 'Yesterday';
  var opts = { month: 'short', day: 'numeric' };
  if (d.getFullYear() !== now.getFullYear()) opts.year = 'numeric';
  try { return d.toLocaleDateString(currentLocale, opts); } catch (e) { return d.toLocaleDateString(); }
}

/** 发送按钮状态：有文字或附件时才可点，且高亮为主色 */
function updateSendState() {
  var btn = $('send-btn');
  if (!btn) return;
  var input = $('msg-input');
  var ready = !!((input && input.value.trim()) || state.pendingAttach);
  btn.disabled = !ready;
  btn.classList.toggle('send-ready', ready);
}
function autoResizeInput(el) {
  el.style.height = 'auto';
  el.style.height = el.scrollHeight + 'px';
}
function getPayloadText(payload) {
  if (payload == null) return '';
  if (typeof payload === 'string') return payload;
  if (typeof payload === 'object' && payload.text) return String(payload.text);
  if (typeof payload === 'object' && payload.content) return String(payload.content);
  if (typeof payload === 'object' && payload.name) return String(payload.name);
  try { return JSON.stringify(payload); } catch (e) { return ''; }
}

/** 会话列表/通知用的短预览 */
function messagePreview(msg) {
  if (!msg) return lang.newMessage || '新消息';
  var mt = msg.message_type || 'text';
  if (mt === 'image') return lang.previewImage || '📷 图片';
  if (mt === 'file' || mt === 'file-meta') return lang.previewFile || '📎 文件';
  if (mt === 'system') return lang.previewSystem || '系统消息';
  var text = getPayloadText(msg.payload);
  if (!text) return lang.newMessage || '新消息';
  return text.length > 80 ? text.slice(0, 79) + '…' : text;
}

/**
 * 应用内新消息 Toast。
 * 条件：设置开启，且（页面在后台 或 当前未打开该会话）。
 * 全局通知中心由后端 SSE 负责，这里只补 Aro 打开时的即时反馈。
 */
function maybeNotifyIncomingMessage(scope, scopeId, msg) {
  if (!state.notifyOnMessage || !msg) return;
  var isActive =
    state.activeKind === scope &&
    state.activeId === scopeId &&
    typeof document !== 'undefined' &&
    !document.hidden;
  if (isActive) return;

  var title = lang.newMessage || '新消息';
  if (scope === 'channel') {
    for (var i = 0; i < state.channels.length; i++) {
      if (state.channels[i].channel_id === scopeId) {
        title = state.channels[i].remote_actor_name ||
          (state.channels[i].remote_actor_url || '').split('/').pop() ||
          title;
        break;
      }
    }
  } else if (scope === 'room') {
    for (var j = 0; j < state.rooms.length; j++) {
      if (state.rooms[j].room_id === scopeId) {
        title = state.rooms[j].name || title;
        break;
      }
    }
  }
  var preview = messagePreview(msg);
  try {
    Tapp.ui.showNotification({ title: title, message: preview, type: 'info' });
  } catch (e) { /* ignore */ }
}
function formatFileSize(bytes) {
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / 1048576).toFixed(1) + ' MB';
}

function getErrorMessage(error) {
  if (!error) return '';
  if (typeof error === 'string') return error;
  if (error.message) return String(error.message);
  if (error.error) return String(error.error);
  try { return JSON.stringify(error); } catch (e) { return ''; }
}

function errorSuffix(error) {
  var message = getErrorMessage(error);
  return message ? ': ' + message : '';
}

function notifyError(title, error) {
  var message = getErrorMessage(error);
  try {
    Tapp.ui.showNotification({ title: title, message: message || undefined, type: 'error' });
  } catch (e) {}
}

function requireAdminAction() {
  if (state.isAdmin) return true;
  notifyError(lang.adminRequired);
  return false;
}

/** 本地化环网类型标签；未知类型原样返回 */
function ringTypeLabel(type) {
  var map = {
    'brew-recommend': lang.ringTypeBrewRecommend,
    'tapp-store': lang.ringTypeTappStore,
    'library-exchange': lang.ringTypeLibraryExchange,
    'instance-directory': lang.ringTypeInstanceDirectory,
  };
  return map[type] || type || '';
}

/** 本地化成员角色标签 */
function roleLabel(role) {
  var map = { owner: lang.roleOwner, admin: lang.roleAdmin, member: lang.roleMember };
  return map[role] || role || '';
}

/** 本地化分享卡片类型标签 */
function shareTypeLabel(type) {
  var map = { tapp: lang.attachTapp, brew: lang.attachBrew, library: lang.attachLibrary, report: lang.attachReport };
  return map[type] || type || '';
}

/**
 * 应用内确认对话框（沙箱 iframe 中原生 confirm() 会被浏览器拦截并静默返回 false）。
 * 返回 Promise<boolean>。
 */
function aroConfirm(message, danger) {
  return new Promise(function (resolve) {
    var overlay = document.createElement('div');
    overlay.className = 'confirm-overlay';
    overlay.innerHTML = '<div class="confirm-dialog">'
      + '<div class="confirm-message">' + esc(message) + '</div>'
      + '<div class="confirm-actions">'
      + '<button class="confirm-btn confirm-btn-cancel">' + esc(lang.confirmCancel || 'Cancel') + '</button>'
      + '<button class="confirm-btn confirm-btn-ok' + (danger ? ' confirm-btn-danger' : '') + '">' + esc(lang.confirmOk || 'OK') + '</button>'
      + '</div></div>';
    var done = function (result) { overlay.remove(); resolve(result); };
    overlay.querySelector('.confirm-btn-cancel').addEventListener('click', function () { done(false); });
    overlay.querySelector('.confirm-btn-ok').addEventListener('click', function () { done(true); });
    overlay.addEventListener('click', function (e) { if (e.target === overlay) done(false); });
    document.body.appendChild(overlay);
    overlay.querySelector('.confirm-btn-ok').focus();
  });
}

function setAdminElementVisible(selector, visible) {
  document.querySelectorAll(selector).forEach(function (el) {
    el.style.display = visible ? '' : 'none';
  });
}

function applyAdminControls() {
  var visible = !!state.isAdmin;
  setAdminElementVisible('#ring-create-open-btn', visible);
  setAdminElementVisible('#ring-sync-btn', visible);
  setAdminElementVisible('#ring-peer-bar', visible);
  setAdminElementVisible('.ring-peer-remove-btn', visible);
  var manageBtn = $('ring-manage-btn');
  var manageWrap = manageBtn ? manageBtn.closest('.manage-wrap') : null;
  if (manageWrap) manageWrap.style.display = visible ? '' : 'none';
  if (!visible) {
    var createDialog = $('ring-create-dialog');
    if (createDialog) createDialog.style.display = 'none';
    var dropdown = $('ring-manage-dropdown');
    if (dropdown) dropdown.classList.remove('open');
  }
}

function applyRoleControls() {
  var privateOnly = !state.isGuest;
  // 访客只有「动态」一个视图，整条顶部导航都没有意义，直接隐藏
  setAdminElementVisible('#aro-nav', privateOnly);
  setAdminElementVisible('#nav-messages', privateOnly);
  setAdminElementVisible('#nav-rings', privateOnly);
  setAdminElementVisible('.feed-nav-item[data-sub="following"]', privateOnly);
  setAdminElementVisible('.feed-nav-item[data-sub="followers"]', privateOnly);
  setAdminElementVisible('.feed-nav-item[data-sub="published"]', privateOnly);
  setAdminElementVisible('.feed-mobile-tab[data-sub="following"]', privateOnly);
  setAdminElementVisible('.feed-mobile-tab[data-sub="followers"]', privateOnly);
  setAdminElementVisible('.feed-mobile-tab[data-sub="published"]', privateOnly);
  setAdminElementVisible('.feed-sidebar-stats', privateOnly);
  setAdminElementVisible('.feed-mobile-stats', privateOnly);
  if (state.isGuest) {
    state.feedSubTab = 'timeline';
    state.currentView = 'feed';
    var followBar = $('feed-follow-bar');
    if (followBar) followBar.style.display = 'none';
    if (typeof closeComposer === 'function') closeComposer();
  }
  if (typeof updateComposeButtonVisibility === 'function') {
    updateComposeButtonVisibility();
  }
}

async function loadUserRole() {
  state.userRole = 'guest';
  state.isGuest = true;
  state.isAdmin = false;
  try {
    if (Tapp.user && typeof Tapp.user.getRole === 'function') {
      state.userRole = (await Tapp.user.getRole()) || 'guest';
      state.isGuest = state.userRole === 'guest';
      state.isAdmin = state.userRole === 'admin';
    } else if (Tapp.user && typeof Tapp.user.isAdmin === 'function') {
      state.isAdmin = !!(await Tapp.user.isAdmin());
      state.userRole = state.isAdmin ? 'admin' : 'user';
      state.isGuest = false;
    }
  } catch (e) {
    state.userRole = 'guest';
    state.isGuest = true;
    state.isAdmin = false;
  }
  applyAdminControls();
  applyRoleControls();
}

function normalizeFederationUrl(value) {
  if (value === null || value === undefined) return '';
  var text = String(value).trim();
  if (!text) return '';
  var lower = text.toLowerCase();
  if (
    lower === 'null' ||
    lower === 'undefined' ||
    lower.indexOf('null/') === 0 ||
    lower.indexOf('undefined/') === 0 ||
    lower.indexOf('://null') !== -1 ||
    lower.indexOf('://undefined') !== -1
  ) {
    return '';
  }
  try {
    var parsed = new URL(text);
    var protocol = parsed.protocol.toLowerCase();
    var host = (parsed.hostname || '').toLowerCase();
    if ((protocol !== 'http:' && protocol !== 'https:') || !host || host === 'null' || host === 'undefined') {
      return '';
    }
    return text;
  } catch (e) {
    return '';
  }
}

function normalizeFederationDomain(value) {
  if (value === null || value === undefined) return '';
  var text = String(value).trim();
  if (!text) return '';
  var lower = text.toLowerCase();
  if (lower === 'null' || lower === 'undefined') return '';
  return text.replace(/^@+/, '');
}

function getIdentityActorUrl() {
  var identity = state.identity || {};
  return normalizeFederationUrl(identity.actor_url) || normalizeFederationUrl(state.localActorUrl);
}

function sanitizeFederationIdentity(identity) {
  if (!identity) return null;
  var clean = {};
  Object.keys(identity).forEach(function (key) {
    clean[key] = identity[key];
  });
  clean.actor_url = normalizeFederationUrl(clean.actor_url);
  clean.domain = normalizeFederationDomain(clean.domain);
  if (!clean.domain && clean.actor_url) {
    try { clean.domain = new URL(clean.actor_url).host; } catch (e) {}
  }
  if (!clean.handle && clean.acct) clean.handle = '@' + String(clean.acct).replace(/^@/, '');
  if (!clean.handle && clean.username && clean.domain) clean.handle = '@' + clean.username + '@' + clean.domain;
  if (!clean.acct && clean.handle) clean.acct = String(clean.handle).replace(/^@/, '');
  if (!clean.actor_url) {
    clean.inbox_url = '';
    clean.outbox_url = '';
    clean.followers_url = '';
    clean.following_url = '';
  }
  return clean;
}

function getIdentityHandle() {
  var identity = state.identity || {};
  if (identity.handle) return identity.handle;
  if (identity.acct) return '@' + identity.acct;
  var domain = normalizeFederationDomain(identity.domain);
  if (identity.username && domain) return '@' + identity.username + '@' + domain;
  return '';
}

function synthesizeFederationIdentityFromUser(user) {
  if (state.identity || !user) return;
  var rawUsername = user.username || user.display_name || user.name || '';
  rawUsername = String(rawUsername).replace(/^@/, '').split('@')[0];
  if (!rawUsername) return;

  var actorUrl = normalizeFederationUrl(user.actor_url) || getIdentityActorUrl();
  var domain = '';
  if (actorUrl) {
    try { domain = new URL(actorUrl).host; } catch (e) {}
  }
  if (!domain) domain = normalizeFederationDomain(user.domain || user.instance_domain) || 'local';

  var acct = rawUsername + '@' + domain;
  state.identity = {
    username: rawUsername,
    domain: domain,
    handle: '@' + acct,
    acct: acct,
    webfinger_resource: 'acct:' + acct,
    actor_url: actorUrl,
    inbox_url: actorUrl ? actorUrl + '/inbox' : '',
    outbox_url: actorUrl ? actorUrl + '/outbox' : '',
    followers_url: actorUrl ? actorUrl + '/followers' : '',
    following_url: actorUrl ? actorUrl + '/following' : '',
    profile_url: ''
  };
  if (actorUrl) state.localActorUrl = actorUrl;
}

function renderFederationIdentity() {
  var identity = sanitizeFederationIdentity(state.identity) || {};
  state.identity = Object.keys(identity).length > 0 ? identity : null;
  var handle = getIdentityHandle();
  var actorUrl = getIdentityActorUrl();
  var visible = !!(handle || actorUrl);

  if (actorUrl) state.localActorUrl = actorUrl;
  else if (state.localActorUrl && !normalizeFederationUrl(state.localActorUrl)) state.localActorUrl = null;

  document.querySelectorAll('[data-fed-profile]').forEach(function (card) {
    card.style.display = visible ? '' : 'none';
    card.classList.toggle('feed-identity-actor-missing', !actorUrl);
    card.querySelectorAll('[data-fed-handle-summary]').forEach(function (handleEl) {
      handleEl.textContent = handle || actorUrl;
    });
    card.querySelectorAll('[data-fed-actor]').forEach(function (actorEl) {
      actorEl.textContent = actorUrl;
      actorEl.disabled = !actorUrl;
      actorEl.style.display = actorUrl ? '' : 'none';
    });
    card.querySelectorAll('[data-fed-toggle-button]').forEach(function (toggleBtn) {
      toggleBtn.style.display = actorUrl ? '' : 'none';
    });
    if (!actorUrl) setFeedProfileExpanded(card, false);
  });

  var profileHandle = $('feed-handle');
  if (profileHandle && handle) profileHandle.textContent = handle;
}

function avatarContentHtml(url, name) {
  var initial = ((name || '?')[0] || '?').toUpperCase();
  if (url) return '<img src="' + esc(url) + '" alt="" />';
  return esc(initial);
}

/** Unwrap getRoomMembers response: { members, total } or legacy bare array. */
function unwrapRoomMembers(res) {
  if (!res) return [];
  if (Array.isArray(res)) return res;
  if (Array.isArray(res.members)) return res.members;
  return [];
}

function sameActorUrl(a, b) {
  var left = normalizeFederationUrl(a) || String(a || '').trim().replace(/\\/+$/, '');
  var right = normalizeFederationUrl(b) || String(b || '').trim().replace(/\\/+$/, '');
  if (!left || !right) return false;
  return left === right || left.replace(/\\/+$/, '') === right.replace(/\\/+$/, '');
}

function findMemberByActor(actorUrl) {
  if (!actorUrl) return null;
  for (var i = 0; i < state.members.length; i++) {
    var m = state.members[i];
    if (sameActorUrl(m.actor_url, actorUrl)) return m;
  }
  return null;
}

function renderFeedProfileUser(user) {
  if (!user) return;
  var name = user.display_name || user.username || '';
  var avatar = user.avatar_url || user.avatar || '';
  // Prefer federation identity avatar when context only has placeholder/empty
  if (!avatar && state.identity && state.identity.avatar_url) {
    avatar = state.identity.avatar_url;
  }
  if (!name && state.identity) {
    name = state.identity.display_name || state.identity.username || name;
  }
  var initial = ((name || user.username || '?')[0] || '?').toUpperCase();
  document.querySelectorAll('[data-feed-avatar]').forEach(function (avatarEl) {
    if (avatar) avatarEl.innerHTML = '<img src="' + esc(avatar) + '" alt="" />';
    else avatarEl.textContent = initial;
  });
  document.querySelectorAll('[data-feed-display-name]').forEach(function (nameEl) {
    nameEl.textContent = name;
  });
  var fallbackHandle = user.username ? '@' + user.username : '';
  if (!state.identity && fallbackHandle) {
    document.querySelectorAll('[data-fed-handle-summary]').forEach(function (handleEl) {
      handleEl.textContent = fallbackHandle;
    });
  }
}

function setFeedProfileExpanded(card, expanded) {
  if (!card) return;
  if (expanded && card.classList.contains('feed-identity-actor-missing')) return;
  card.classList.toggle('feed-profile-expanded', !!expanded);
  var summary = card.querySelector('[data-fed-toggle]');
  if (summary) summary.setAttribute('aria-expanded', expanded ? 'true' : 'false');
  card.querySelectorAll('[data-fed-toggle-button]').forEach(function (toggleBtn) {
    toggleBtn.setAttribute('title', expanded ? (lang.collapseDetails || '收起') : (lang.expandDetails || '展开'));
  });
}

function isTabletFeedProfileCard(card) {
  return !!(card && card.closest('.feed-sidebar') && window.matchMedia && window.matchMedia('(min-width: 769px) and (max-width: 1024px)').matches);
}

function closeFeedProfilePopovers(exceptCard) {
  document.querySelectorAll('.feed-profile-popover-open').forEach(function (card) {
    if (card !== exceptCard) {
      card.classList.remove('feed-profile-popover-open');
      setFeedProfileExpanded(card, false);
    }
  });
}

function setFeedProfilePopoverOpen(card, open) {
  if (!card) return;
  if (open) {
    closeFeedProfilePopovers(card);
    card.classList.add('feed-profile-popover-open');
    setFeedProfileExpanded(card, false);
  } else {
    card.classList.remove('feed-profile-popover-open');
    setFeedProfileExpanded(card, false);
  }
}

function toggleFeedProfileDetails(card) {
  if (!card || card.classList.contains('feed-identity-actor-missing')) return;
  setFeedProfileExpanded(card, !card.classList.contains('feed-profile-expanded'));
}

function toggleFeedProfileSummary(card) {
  if (!card) return;
  if (isTabletFeedProfileCard(card)) {
    setFeedProfilePopoverOpen(card, !card.classList.contains('feed-profile-popover-open'));
    return;
  }
  toggleFeedProfileDetails(card);
}

async function loadFederationIdentity() {
  if (state.isGuest) {
    try {
      var guestUser = await Tapp.context.getUser();
      synthesizeFederationIdentityFromUser(guestUser);
    } catch (e) {}
    renderFederationIdentity();
    return;
  }
  if (!Tapp.federation || typeof Tapp.federation.getIdentity !== 'function') {
    try {
      var fallbackUser = await Tapp.context.getUser();
      synthesizeFederationIdentityFromUser(fallbackUser);
    } catch (e) {}
    renderFederationIdentity();
    return;
  }
  try {
    var identity = await Tapp.federation.getIdentity();
    if (identity) {
      state.identity = sanitizeFederationIdentity(identity);
      var actorUrl = getIdentityActorUrl();
      if (actorUrl) state.localActorUrl = actorUrl;
    }
  } catch (e) {
    console.warn('[Aro] federation identity unavailable:', e);
  }
  if (!state.identity) {
    try {
      var fallbackUser2 = await Tapp.context.getUser();
      synthesizeFederationIdentityFromUser(fallbackUser2);
    } catch (e2) {}
  }
  renderFederationIdentity();
}

function fallbackCopyText(text) {
  var area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', 'readonly');
  area.style.position = 'fixed';
  area.style.left = '-9999px';
  document.body.appendChild(area);
  area.select();
  area.setSelectionRange(0, text.length);
  var ok = false;
  try {
    ok = document.execCommand('copy');
  } catch (e) {
    ok = false;
  } finally {
    area.remove();
  }
  return ok;
}

async function copyFederationIdentity(kind) {
  var text = kind === 'actor' ? getIdentityActorUrl() : getIdentityHandle();
  if (!text) return;
  var ok = false;
  // Tapp 运行在 opaque-origin 的沙箱 iframe 中，异步 Clipboard API 会被
  // 浏览器以 NotAllowedError 拒绝，因此拒绝后必须回退到 execCommand。
  try {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(text);
      ok = true;
    }
  } catch (e) {
    ok = false;
  }
  if (!ok) ok = fallbackCopyText(text);
  if (ok) {
    try { Tapp.ui.showNotification({ title: lang.copied, message: text, type: 'success' }); } catch (e2) {}
  } else {
    try { Tapp.ui.showNotification({ title: lang.copyFail, type: 'error' }); } catch (e3) {}
  }
}

function isLocalActor(actor) {
  if (!actor) return false;
  var localActor = getIdentityActorUrl();
  if (localActor && sameActorUrl(actor, localActor)) return true;
  if (state.localActorUrl && sameActorUrl(actor, state.localActorUrl)) return true;
  if (state.activeKind === 'channel' && state.channelDetail && state.channelDetail.remote_actor_url) {
    // In a 1:1 channel, anything that is not the remote peer is local
    return !sameActorUrl(actor, state.channelDetail.remote_actor_url);
  }
  if (state.activeKind === 'room' && state.members.length > 0) {
    var member = findMemberByActor(actor);
    if (member) return !!member.is_local;
  }
  return String(actor).indexOf('myriad.local') !== -1;
}

function applyLabels() {
  var el;
  el = $('nav-messages-label'); if (el) el.textContent = lang.navMessages;
  el = $('nav-rings-label'); if (el) el.textContent = lang.navRings;
  el = document.querySelector('.sidebar-title'); if (el) el.textContent = lang.title;
  el = document.querySelector('.empty-text'); if (el) el.textContent = lang.selectHint;
  el = $('msg-input'); if (el) el.placeholder = lang.typing;
  el = $('feed-nav-timeline'); if (el) el.textContent = lang.feedTimeline;
  el = $('feed-nav-following'); if (el) el.textContent = lang.feedFollowing;
  el = $('feed-nav-followers'); if (el) el.textContent = lang.feedFollowers;
  el = $('feed-nav-published'); if (el) el.textContent = lang.feedPublished;
  el = $('feed-tab-timeline'); if (el) el.textContent = lang.feedTimeline;
  el = $('feed-tab-following'); if (el) el.textContent = lang.feedFollowing;
  el = $('feed-tab-followers'); if (el) el.textContent = lang.feedFollowers;
  el = $('feed-tab-published'); if (el) el.textContent = lang.feedPublished;
  el = $('feed-lbl-following'); if (el) el.textContent = lang.feedFollowing;
  el = $('feed-lbl-followers'); if (el) el.textContent = lang.feedFollowers;
  el = $('feed-lbl-published'); if (el) el.textContent = lang.feedPublished;
  el = $('feed-mobile-lbl-following'); if (el) el.textContent = lang.feedFollowing;
  el = $('feed-mobile-lbl-followers'); if (el) el.textContent = lang.feedFollowers;
  el = $('feed-mobile-lbl-published'); if (el) el.textContent = lang.feedPublished;
  el = $('feed-follow-input'); if (el) el.placeholder = lang.followPlaceholder;
  el = $('feed-follow-btn'); if (el) el.textContent = lang.followBtn;
  el = $('feed-compose-btn-label'); if (el) el.textContent = lang.composePost || 'Post';
  el = $('feed-compose-btn'); if (el) el.setAttribute('title', lang.composePost || 'Post');
  el = $('feed-compose-text'); if (el) el.placeholder = lang.composePlaceholder || '';
  el = $('feed-compose-image-label'); if (el) el.textContent = lang.composeAddImage || 'Image';
  el = $('feed-compose-video-label'); if (el) el.textContent = lang.composeAddVideo || 'Video';
  el = $('feed-compose-cancel'); if (el) el.textContent = lang.composeCancel || 'Cancel';
  el = $('feed-compose-publish'); if (el) el.textContent = lang.composePublish || 'Publish';
  el = $('refresh-feed-btn'); if (el) el.setAttribute('title', lang.refresh);
  el = $('refresh-feed-mobile-btn'); if (el) el.setAttribute('title', lang.refresh);
  if (typeof updateComposeButtonVisibility === 'function') updateComposeButtonVisibility();
  document.querySelectorAll('[data-copy-fed]').forEach(function (node) { node.setAttribute('title', lang.copy); });
  document.querySelectorAll('[data-fed-profile]').forEach(function (card) {
    setFeedProfileExpanded(card, card.classList.contains('feed-profile-expanded'));
  });
  updateFeedHeader();
  el = $('ring-sidebar-title'); if (el) el.textContent = lang.navRings;
  el = $('ring-select-hint'); if (el) el.textContent = lang.selectRing;
  el = $('ring-create-title'); if (el) el.textContent = lang.createRingTitle;
  el = $('ring-name-input'); if (el) el.placeholder = lang.ringNamePlaceholder;
  el = $('create-ring-btn'); if (el) el.textContent = lang.createRingBtn;
  el = $('ring-peer-input'); if (el) el.placeholder = lang.addPeerPlaceholder;
  el = $('ring-add-peer-btn'); if (el) el.textContent = lang.addPeerBtn;
  el = $('ring-type-opt-brew'); if (el) el.textContent = lang.ringTypeBrewRecommend;
  el = $('ring-type-opt-tapp'); if (el) el.textContent = lang.ringTypeTappStore;
  el = $('ring-type-opt-library'); if (el) el.textContent = lang.ringTypeLibraryExchange;
  el = $('ring-type-opt-instance'); if (el) el.textContent = lang.ringTypeInstanceDirectory;
}

function applyDialogLabels() {
  var el;
  el = $('create-channel-input'); if (el) el.placeholder = lang.channelPlaceholder;
  el = $('create-room-input'); if (el) el.placeholder = lang.roomPlaceholder;
  el = $('create-channel-btn'); if (el) el.textContent = lang.createChannel;
  el = $('create-room-btn'); if (el) el.textContent = lang.createRoom;
  el = $('create-tab-channel'); if (el) el.textContent = lang.newChannel;
  el = $('create-tab-room'); if (el) el.textContent = lang.newRoom;
  el = $('invite-input'); if (el) el.placeholder = lang.invitePlaceholder;
  el = $('invite-pop-contacts-label'); if (el) el.textContent = lang.inviteFromContacts;
  el = $('invite-pop-manual-label'); if (el) el.textContent = lang.inviteManual;
  el = $('edit-room-title'); if (el) el.textContent = lang.editRoom;
  el = $('edit-name-label'); if (el) el.textContent = lang.roomName;
  el = $('edit-desc-label'); if (el) el.textContent = lang.roomDesc;
  el = $('edit-room-save'); if (el) el.textContent = lang.save;
}
`

const PAGE_MOD_ATTACHMENTS = `\
// ==================== Attachment Menu ====================
var _attachMenu = null;
var MAX_ATTACH_SIZE = 10 * 1024 * 1024; // 10MB

function toggleAttachMenu() {
  if (_attachMenu) { closeAttachMenu(); return; }
  var wrap = $('input-bar');
  if (!wrap) return;
  wrap.style.position = 'relative';
  var btn = $('attach-btn');
  if (btn) btn.classList.add('attach-btn-active');

  var menu = document.createElement('div');
  menu.className = 'attach-menu';
  menu.innerHTML =
    '<button class="attach-menu-item" data-attach="image"><div class="attach-menu-icon attach-icon-image"><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg></div>' + esc(lang.attachImage) + '</button>'
    + '<button class="attach-menu-item" data-attach="file"><div class="attach-menu-icon attach-icon-file"><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6"/></svg></div>' + esc(lang.attachFile) + '</button>'
    + '<button class="attach-menu-item" data-attach="tapp"><div class="attach-menu-icon attach-icon-tapp"><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg></div>' + esc(lang.attachTapp) + '</button>'
    + '<button class="attach-menu-item" data-attach="brew"><div class="attach-menu-icon attach-icon-brew"><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M18 8h1a4 4 0 010 8h-1M2 8h16v9a4 4 0 01-4 4H6a4 4 0 01-4-4V8z"/><path d="M6 1v3M10 1v3M14 1v3"/></svg></div>' + esc(lang.attachBrew) + '</button>'
    + '<button class="attach-menu-item" data-attach="library"><div class="attach-menu-icon attach-icon-library"><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 19.5A2.5 2.5 0 016.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/></svg></div>' + esc(lang.attachLibrary) + '</button>'
    + '<button class="attach-menu-item" data-attach="report"><div class="attach-menu-icon attach-icon-report"><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6M16 13H8M16 17H8M10 9H8"/></svg></div>' + esc(lang.attachReport) + '</button>';

  menu.addEventListener('click', function (e) {
    var item = e.target.closest('[data-attach]');
    if (!item) return;
    var type = item.dataset.attach;
    closeAttachMenu();
    if (type === 'image') { var inp = $('attach-image-input'); if (inp) inp.click(); }
    else if (type === 'file') { var inp2 = $('attach-file-input'); if (inp2) inp2.click(); }
    else pickFedContent(type);
  });

  wrap.appendChild(menu);
  _attachMenu = menu;

  // Close on outside click
  setTimeout(function () {
    document.addEventListener('click', _attachOutsideClick);
  }, 0);
}

function _attachOutsideClick(e) {
  if (_attachMenu && !_attachMenu.contains(e.target) && e.target.id !== 'attach-btn' && !e.target.closest('#attach-btn')) {
    closeAttachMenu();
  }
}

function closeAttachMenu() {
  if (_attachMenu) { _attachMenu.remove(); _attachMenu = null; }
  var btn = $('attach-btn');
  if (btn) btn.classList.remove('attach-btn-active');
  document.removeEventListener('click', _attachOutsideClick);
}

function handleFileSelect(file, forceType) {
  if (!file) return;
  if (file.size > MAX_ATTACH_SIZE) {
    try { Tapp.ui.showNotification({ title: lang.fileTooLarge, type: 'error' }); } catch (e) { /* ignore */ }
    return;
  }
  var type = forceType || (file.type.startsWith('image/') ? 'image' : 'file');
  var reader = new FileReader();
  reader.onload = function () {
    setPendingAttach({ type: type, data: reader.result, name: file.name, size: file.size, mime: file.type });
  };
  reader.readAsDataURL(file);
}

function pickFedContent(type) {
  var icons = { tapp: SVG_ICONS.tapp, brew: SVG_ICONS.brew, library: SVG_ICONS.library, report: SVG_ICONS.report };
  var titles = { tapp: lang.selectTapp, brew: lang.selectBrew, library: lang.selectLibrary, report: lang.selectReport };
  var iconColors = { tapp: 'attach-icon-tapp', brew: 'attach-icon-brew', library: 'attach-icon-library', report: 'attach-icon-report' };

  if (type === 'tapp') { openTappPicker(icons, titles, iconColors); return; }
  if (type === 'brew') { openBrewPicker(icons, titles, iconColors); return; }
  if (type === 'library') { openLibraryPicker(icons, titles, iconColors); return; }
  if (type === 'report') { openReportPicker(icons, titles, iconColors); return; }
}

/* ----- Shared overlay helpers ----- */
function createPickerOverlay(type, icons, titles, iconColors) {
  var overlay = document.createElement('div');
  overlay.className = 'picker-overlay';
  overlay.innerHTML =
    '<div class="picker-sheet">'
    + '<div class="picker-header">'
    + '<div class="picker-header-icon ' + esc(iconColors[type]) + '">' + icons[type] + '</div>'
    + '<div class="picker-header-title">' + esc(titles[type]) + '</div>'
    + '<button class="picker-close-btn">&times;</button>'
    + '</div>'
    + '<div class="picker-search"><input placeholder="' + esc(lang.pickerSearchPlaceholder) + '" /></div>'
    + '<div class="picker-body"></div>'
    + '<div class="picker-footer">'
    + '<button class="picker-footer-btn picker-btn-cancel">' + esc(lang.pickerCancel) + '</button>'
    + '<button class="picker-footer-btn picker-btn-confirm" disabled>' + esc(lang.pickerConfirm) + '</button>'
    + '</div>'
    + '</div>';
  overlay.querySelector('.picker-close-btn').addEventListener('click', function () { overlay.remove(); });
  overlay.querySelector('.picker-btn-cancel').addEventListener('click', function () { overlay.remove(); });
  overlay.addEventListener('click', function (e) { if (e.target === overlay) overlay.remove(); });
  document.body.appendChild(overlay);
  return overlay;
}

function showPickerLoading(body) {
  body.innerHTML = '<div class="picker-loading"><div class="picker-loading-spinner"></div>' + esc(lang.pickerLoading) + '</div>';
}
function showPickerEmpty(body) {
  body.innerHTML = '<div class="picker-empty">' + esc(lang.pickerEmpty) + '</div>';
}

function bindPickerSearch(overlay, allItems, renderFn, filterFn) {
  var searchInput = overlay.querySelector('.picker-search input');
  if (!searchInput) return;
  searchInput.addEventListener('input', function () {
    var q = this.value.trim().toLowerCase();
    if (!q) { renderFn(allItems); return; }
    renderFn(allItems.filter(function (item) { return filterFn(item, q); }));
  });
}

function bindPickerItems(body, items, confirmBtn, onSelect) {
  body.querySelectorAll('.picker-item').forEach(function (el) {
    el.addEventListener('click', function () {
      body.querySelectorAll('.picker-item').forEach(function (e) { e.classList.remove('selected'); });
      el.classList.add('selected');
      onSelect(items[parseInt(el.dataset.idx)]);
      confirmBtn.disabled = false;
    });
  });
}

/* ----- Tapp picker (real list from SDK) ----- */
function openTappPicker(icons, titles, iconColors) {
  var type = 'tapp';
  var overlay = createPickerOverlay(type, icons, titles, iconColors);
  var body = overlay.querySelector('.picker-body');
  var confirmBtn = overlay.querySelector('.picker-btn-confirm');
  var selectedTapp = null;
  var allTapps = [];

  showPickerLoading(body);

  Tapp.tappList.list().then(function (tapps) {
    allTapps = tapps || [];
    renderTappItems(allTapps);
  }).catch(function () { showPickerEmpty(body); });

  function renderTappItems(items) {
    if (!items.length) { showPickerEmpty(body); return; }
    body.innerHTML = items.map(function (t, i) {
      var meta = t.version || '';
      if (t.status) meta += (meta ? ' · ' : '') + t.status;
      return '<button class="picker-item" data-idx="' + i + '">'
        + '<div class="picker-item-icon" style="background:rgba(var(--tapp-primary-rgb,100,100,255),.1);color:var(--tapp-primary,#6366f1)">'
        + (t.iconSvg ? t.iconSvg : (t.icon ? '<img src="' + esc(t.icon) + '" style="width:100%;height:100%;object-fit:cover;border-radius:8px" />' : SVG_ICONS.tapp))
        + '</div>'
        + '<div class="picker-item-body"><div class="picker-item-name">' + esc(t.name) + '</div>'
        + '<div class="picker-item-meta">' + esc(t.id + (meta ? ' · ' + meta : '')) + '</div>'
        + (t.description ? '<div class="picker-item-meta">' + esc(t.description) + '</div>' : '')
        + '</div><div class="picker-item-check">✓</div></button>';
    }).join('');
    bindPickerItems(body, items, confirmBtn, function (t) { selectedTapp = t; });
  }

  bindPickerSearch(overlay, allTapps, renderTappItems, function (t, q) {
    return (t.name || '').toLowerCase().indexOf(q) !== -1
      || (t.id || '').toLowerCase().indexOf(q) !== -1
      || (t.description || '').toLowerCase().indexOf(q) !== -1;
  });

  confirmBtn.addEventListener('click', function () {
    if (!selectedTapp) return;
    setPendingAttach({ type: type, name: selectedTapp.name, desc: selectedTapp.description || selectedTapp.id, icon: icons[type], label: 'Tapp', tappId: selectedTapp.id, tappVersion: selectedTapp.version || '', tappIcon: selectedTapp.iconSvg || selectedTapp.icon || '' });
    overlay.remove();
  });
}

/* ----- Brew picker (real list from SDK) ----- */
function openBrewPicker(icons, titles, iconColors) {
  var type = 'brew';
  var overlay = createPickerOverlay(type, icons, titles, iconColors);
  var body = overlay.querySelector('.picker-body');
  var confirmBtn = overlay.querySelector('.picker-btn-confirm');
  var selectedBrew = null;
  var allBrews = [];

  showPickerLoading(body);

  Tapp.brewList.list({ limit: 50 }).then(function (res) {
    allBrews = (res && res.items) || [];
    renderBrewItems(allBrews);
  }).catch(function () { showPickerEmpty(body); });

  function renderBrewItems(items) {
    if (!items.length) { showPickerEmpty(body); return; }
    body.innerHTML = items.map(function (b, i) {
      var meta = b.source_name || '';
      if (b.author) meta += (meta ? ' · ' : '') + b.author;
      if (b.published_at) meta += (meta ? ' · ' : '') + new Date(b.published_at).toLocaleDateString();
      return '<button class="picker-item" data-idx="' + i + '">'
        + '<div class="picker-item-icon" style="background:rgba(34,197,94,.1);color:#22c55e">'
        + (b.image ? '<img src="' + esc(b.image) + '" style="width:100%;height:100%;object-fit:cover;border-radius:8px" />' : (b.source_icon ? '<img src="' + esc(b.source_icon) + '" style="width:100%;height:100%;object-fit:cover;border-radius:8px" />' : SVG_ICONS.brew))
        + '</div>'
        + '<div class="picker-item-body"><div class="picker-item-name">' + esc(b.title) + '</div>'
        + (meta ? '<div class="picker-item-meta">' + esc(meta) + '</div>' : '')
        + (b.summary ? '<div class="picker-item-meta" style="display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;white-space:normal">' + esc(b.summary) + '</div>' : '')
        + '</div><div class="picker-item-check">✓</div></button>';
    }).join('');
    bindPickerItems(body, items, confirmBtn, function (b) { selectedBrew = b; });
  }

  bindPickerSearch(overlay, allBrews, renderBrewItems, function (b, q) {
    return (b.title || '').toLowerCase().indexOf(q) !== -1
      || (b.author || '').toLowerCase().indexOf(q) !== -1
      || (b.source_name || '').toLowerCase().indexOf(q) !== -1
      || (b.summary || '').toLowerCase().indexOf(q) !== -1;
  });

  confirmBtn.addEventListener('click', function () {
    if (!selectedBrew) return;
    var desc = selectedBrew.source_name || '';
    if (selectedBrew.author) desc += (desc ? ' · ' : '') + selectedBrew.author;
    setPendingAttach({ type: type, name: selectedBrew.title, desc: desc, icon: icons[type], label: 'Brew', brewId: selectedBrew.id, brewLink: selectedBrew.link });
    overlay.remove();
  });
}

/* ----- Library picker (platform data) ----- */
function openLibraryPicker(icons, titles, iconColors) {
  var type = 'library';
  var overlay = createPickerOverlay(type, icons, titles, iconColors);
  var sheet = overlay.querySelector('.picker-sheet');
  var body = overlay.querySelector('.picker-body');
  var confirmBtn = overlay.querySelector('.picker-btn-confirm');
  var selectedItem = null;

  showPickerLoading(body);

  // Insert platform tabs before search
  var searchDiv = overlay.querySelector('.picker-search');
  var tabsDiv = document.createElement('div');
  tabsDiv.className = 'picker-tabs';
  sheet.insertBefore(tabsDiv, searchDiv);

  var allItems = [];
  var activePlatform = null;

  Tapp.platform.listEnabled().then(function (platforms) {
    if (!platforms || !platforms.length) { showPickerEmpty(body); return; }
    tabsDiv.innerHTML = platforms.map(function (p) {
      return '<button class="picker-tab" data-pid="' + esc(p.id) + '">' + (p.icon ? '<span style="margin-right:3px">' + esc(p.icon) + '</span>' : '') + esc(p.name) + '</button>';
    }).join('');
    selectPlatform(platforms[0].id);
    tabsDiv.addEventListener('click', function (e) {
      var tab = e.target.closest('.picker-tab');
      if (!tab) return;
      selectPlatform(tab.dataset.pid);
    });
  }).catch(function () { showPickerEmpty(body); });

  function selectPlatform(pid) {
    activePlatform = pid;
    allItems = [];
    selectedItem = null;
    confirmBtn.disabled = true;
    tabsDiv.querySelectorAll('.picker-tab').forEach(function (t) {
      t.classList.toggle('active', t.dataset.pid === pid);
    });
    showPickerLoading(body);
    Tapp.platform.getData(pid, { limit: 50 }).then(function (res) {
      allItems = (res && res.items) || [];
      renderLibraryItems(allItems);
    }).catch(function () { showPickerEmpty(body); });
  }

  function renderLibraryItems(items) {
    if (!items.length) { showPickerEmpty(body); return; }
    body.innerHTML = items.map(function (item, i) {
      var name = item.title || item.name || item.id || ('Item ' + (i + 1));
      var meta = item.platform || item.type || '';
      if (item.score !== undefined && item.score !== null) meta += (meta ? ' · ' : '') + '★ ' + item.score;
      if (item.year) meta += (meta ? ' · ' : '') + item.year;
      return '<button class="picker-item" data-idx="' + i + '">'
        + '<div class="picker-item-icon" style="background:rgba(168,85,247,.1);color:#a855f7">' + (item.image ? '<img src="' + esc(item.image) + '" style="width:100%;height:100%;object-fit:cover;border-radius:8px" />' : SVG_ICONS.library) + '</div>'
        + '<div class="picker-item-body"><div class="picker-item-name">' + esc(name) + '</div>'
        + (meta ? '<div class="picker-item-meta">' + esc(meta) + '</div>' : '')
        + '</div><div class="picker-item-check">✓</div></button>';
    }).join('');
    bindPickerItems(body, items, confirmBtn, function (item) { selectedItem = item; });
  }

  bindPickerSearch(overlay, allItems, renderLibraryItems, function (item, q) {
    return ((item.title || item.name || item.id || '').toLowerCase()).indexOf(q) !== -1;
  });

  confirmBtn.addEventListener('click', function () {
    if (!selectedItem) return;
    var name = selectedItem.title || selectedItem.name || selectedItem.id || 'Unknown';
    var desc = activePlatform || '';
    if (selectedItem.score !== undefined) desc += (desc ? ' · ' : '') + '★ ' + selectedItem.score;
    setPendingAttach({ type: type, name: name, desc: desc, icon: icons[type], label: lang.attachLibrary, platformId: activePlatform, itemId: selectedItem.id });
    overlay.remove();
  });
}

/* ----- Report picker ----- */
function openReportPicker(icons, titles, iconColors) {
  var type = 'report';
  var overlay = createPickerOverlay(type, icons, titles, iconColors);
  var body = overlay.querySelector('.picker-body');
  var confirmBtn = overlay.querySelector('.picker-btn-confirm');
  var selectedReport = null;
  var allReports = [];

  showPickerLoading(body);

  Tapp.report.listReports().then(function (res) {
    allReports = (res && res.reports) || [];
    renderReportItems(allReports);
  }).catch(function () { showPickerEmpty(body); });

  function renderReportItems(reports) {
    if (!reports.length) { showPickerEmpty(body); return; }
    body.innerHTML = reports.map(function (r, i) {
      var name = r.summary || r.type || ('Report ' + (i + 1));
      var meta = '';
      if (r.platform) meta += r.platform;
      if (r.type) meta += (meta ? ' · ' : '') + r.type;
      if (r.createdAt) meta += (meta ? ' · ' : '') + new Date(r.createdAt).toLocaleDateString();
      return '<button class="picker-item" data-idx="' + i + '">'
        + '<div class="picker-item-icon" style="background:rgba(239,68,68,.1);color:#ef4444">' + SVG_ICONS.report + '</div>'
        + '<div class="picker-item-body"><div class="picker-item-name">' + esc(name) + '</div>'
        + (meta ? '<div class="picker-item-meta">' + esc(meta) + '</div>' : '')
        + '</div><div class="picker-item-check">✓</div></button>';
    }).join('');
    bindPickerItems(body, reports, confirmBtn, function (r) { selectedReport = r; });
  }

  bindPickerSearch(overlay, allReports, renderReportItems, function (r, q) {
    return ((r.summary || '') + ' ' + (r.type || '') + ' ' + (r.platform || '')).toLowerCase().indexOf(q) !== -1;
  });

  confirmBtn.addEventListener('click', function () {
    if (!selectedReport) return;
    var name = selectedReport.summary || selectedReport.type || 'Report';
    var desc = '';
    if (selectedReport.platform) desc = selectedReport.platform;
    if (selectedReport.createdAt) desc += (desc ? ' · ' : '') + new Date(selectedReport.createdAt).toLocaleDateString();
    setPendingAttach({ type: type, name: name, desc: desc, icon: icons[type], label: lang.attachReport, reportId: selectedReport.id });
    overlay.remove();
  });
}

function setPendingAttach(attach) {
  state.pendingAttach = attach;
  renderAttachPreview();
  updateSendState();
}

function clearPendingAttach() {
  state.pendingAttach = null;
  var preview = $('attach-preview');
  if (preview) { preview.style.display = 'none'; preview.innerHTML = ''; }
  // Reset file inputs
  var fi = $('attach-file-input'); if (fi) fi.value = '';
  var ii = $('attach-image-input'); if (ii) ii.value = '';
  updateSendState();
}

function renderAttachPreview() {
  var preview = $('attach-preview');
  if (!preview || !state.pendingAttach) return;
  var a = state.pendingAttach;
  var html = '';
  if (a.type === 'image' && a.data) {
    html += '<div class="attach-preview-thumb"><img src="' + esc(a.data) + '" alt="" /></div>';
  } else if (a.type === 'file') {
    html += '<div class="attach-preview-icon attach-icon-file" style="background:rgba(245,158,11,.1)">' + SVG_ICONS.file + '</div>';
  } else {
    var iconBg = { tapp: 'rgba(var(--tapp-primary-rgb,100,100,255),.1)', brew: 'rgba(34,197,94,.1)', library: 'rgba(168,85,247,.1)', report: 'rgba(239,68,68,.1)' };
    html += '<div class="attach-preview-icon" style="background:' + (iconBg[a.type] || 'rgba(128,128,128,.06)') + '">' + (a.icon || SVG_ICONS.file) + '</div>';
  }
  html += '<div class="attach-preview-info">'
    + '<div class="attach-preview-name">' + esc(a.name || '') + '</div>'
    + '<div class="attach-preview-meta">' + (a.size ? formatFileSize(a.size) : (a.label || a.type)) + '</div>'
    + '</div>'
    + '<button class="attach-preview-remove" id="attach-remove">&times;</button>';
  preview.innerHTML = html;
  preview.style.display = 'flex';
  var removeBtn = $('attach-remove');
  if (removeBtn) removeBtn.addEventListener('click', clearPendingAttach);
}
`

const PAGE_MOD_CHAT = `\
// ==================== Render: Conversation List ====================
function renderConvList() {
  var list = $('conv-list');
  if (!list) return;

  var items = [];
  state.channels.forEach(function (ch) {
    items.push({
      kind: 'channel', id: ch.channel_id,
      name: ch.remote_actor_name || (ch.remote_actor_url || '').split('/').pop() || '?',
      avatar: ch.remote_actor_avatar || '',
      preview: lang.dm,
      unread: ch.unread_count || 0,
      status: ch.status,
      initiatedBy: ch.initiated_by,
      sortTime: ch.last_activity_at || ch.created_at || '',
    });
  });
  state.rooms.forEach(function (rm) {
    items.push({
      kind: 'room', id: rm.room_id,
      name: rm.name || '?',
      avatar: rm.avatar_url || '',
      preview: (rm.member_count || 0) + ' ' + lang.members,
      unread: rm.unread_count || 0,
      sortTime: rm.last_message_at || rm.created_at || '',
    });
  });
  items.sort(function (a, b) { return (b.sortTime || '').localeCompare(a.sortTime || ''); });

  if (items.length === 0) {
    list.innerHTML = '<div class="conv-empty conv-empty-fill"><span>' + esc(lang.noConv)
      + '<br><span style="font-size:11px;opacity:.75">' + esc(lang.noConvHint || '') + '</span></span></div>';
    return;
  }

  var html = '';
  items.forEach(function (item) {
    var isActive = item.id === state.activeId;
    var avatarClass = item.kind === 'channel' ? 'avatar-channel' : 'avatar-room';
    var rel = item.sortTime ? relTimeStr(item.sortTime) : '';
    html += '<button class="conv-item' + (isActive ? ' conv-active' : '') + (item.unread > 0 ? ' conv-unread' : '') + '" data-kind="' + item.kind + '" data-id="' + esc(item.id) + '">'
      + '<span class="conv-accent" aria-hidden="true"></span>'
      + '<div class="conv-avatar ' + avatarClass + '">' + avatarContentHtml(item.avatar || '', item.name) + '</div>'
      + '<div class="conv-info">'
      + '<div class="conv-top">'
      + '<span class="conv-name">' + esc(item.name) + '</span>'
      + (rel ? '<span class="conv-time">' + esc(rel) + '</span>' : '')
      + '</div>'
      + '<div class="conv-bottom">'
      + '<span class="conv-preview">' + esc(item.preview) + '</span>';
    if (item.unread > 0) {
      html += '<span class="conv-badge">' + (item.unread > 9 ? '9+' : item.unread) + '</span>';
    }
    if (item.status === 'closed') {
      html += '<span class="conv-closed">' + esc(lang.closed) + '</span>';
    }
    if (item.status === 'pending' && item.initiatedBy === 'remote') {
      html += '<span class="conv-pending">' + esc(lang.pending) + '</span>';
    }
    html += '</div></div></button>';
  });
  list.innerHTML = html;

  list.querySelectorAll('.conv-item').forEach(function (el) {
    el.addEventListener('click', function () {
      openConversation(el.dataset.kind, el.dataset.id);
    });
  });
}

// ==================== Render: Pinned Bar ====================
state.pinnedBarDismissed = false;

function renderPinnedBar() {
  var bar = $('pinned-bar');
  if (!bar) return;
  if (state.pinnedBarDismissed) { bar.style.display = 'none'; return; }
  var pinned = [];
  for (var i = 0; i < state.messages.length; i++) {
    if (state.messages[i].is_pinned) pinned.push(state.messages[i]);
  }
  if (pinned.length === 0) { bar.style.display = 'none'; return; }
  var last = pinned[pinned.length - 1];
  var text = getPayloadText(last.payload) || '';
  if (!text && last.payload) {
    text = last.payload.title || last.payload.filename || '';
  }
  bar.style.display = '';
  bar.innerHTML = '<span class="pinned-bar-icon"><svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M9 11V4a1 1 0 011-1h4a1 1 0 011 1v7"/><path d="M5 17h14"/><path d="M7 11l-2 6h14l-2-6"/></svg></span>'
    + '<div class="pinned-bar-body">'
    + '<span class="pinned-bar-label">' + esc(lang.pinnedMsg) + (pinned.length > 1 ? ' (' + pinned.length + ')' : '') + '</span>'
    + '<span class="pinned-bar-text">' + esc(text) + '</span>'
    + '</div>'
    + '<button class="pinned-bar-close" id="pinned-bar-close">&times;</button>';
  var closeBtn = $('pinned-bar-close');
  if (closeBtn) closeBtn.addEventListener('click', function (e) {
    e.stopPropagation();
    state.pinnedBarDismissed = true;
    bar.style.display = 'none';
  });
  bar.onclick = function () {
    var msgEl = document.querySelector('[data-msg-id="' + last.message_id + '"]');
    if (msgEl) msgEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
  };
}

// ==================== Message Context Menu ====================
var _msgMenu = null;
var _longPressTimer = null;

function closeMsgMenu() {
  if (_msgMenu) { _msgMenu.remove(); _msgMenu = null; }
}

function showMsgMenu(msgEl, x, y) {
  closeMsgMenu();
  var msgId = msgEl.dataset.msgId;
  if (!msgId) return;
  var msg = null;
  for (var i = 0; i < state.messages.length; i++) {
    if (state.messages[i].message_id === msgId) { msg = state.messages[i]; break; }
  }
  if (!msg) return;

  var isPinned = !!msg.is_pinned;
  var pinSvg = '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M9 11V4a1 1 0 011-1h4a1 1 0 011 1v7"/><path d="M5 17h14"/><path d="M7 11l-2 6h14l-2-6"/></svg>';
  var quoteSvg = '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V21z"/><path d="M15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2h.75c0 2.25.25 4-2.75 4v3z"/></svg>';
  var forwardSvg = '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 01-.9 3.8 8.5 8.5 0 01-7.6 4.7 8.38 8.38 0 01-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 01-.9-3.8 8.5 8.5 0 014.7-7.6 8.38 8.38 0 013.8-.9h.5a8.48 8.48 0 018 8v.5z"/><path d="M14 9l3 3-3 3"/><path d="M17 12H9"/></svg>';

  var menu = document.createElement('div');
  menu.className = 'msg-ctx-menu';
  menu.innerHTML =
    '<button class="msg-ctx-item" data-action="pin">' + pinSvg + '<span>' + (isPinned ? esc(lang.msgUnpin) : esc(lang.msgPin)) + '</span></button>'
    + '<button class="msg-ctx-item" data-action="quote">' + quoteSvg + '<span>' + esc(lang.msgQuote) + '</span></button>'
    + '<button class="msg-ctx-item" data-action="forward">' + forwardSvg + '<span>' + esc(lang.msgForward) + '</span></button>';

  document.body.appendChild(menu);
  var mw = menu.offsetWidth, mh = menu.offsetHeight;
  var ww = window.innerWidth, wh = window.innerHeight;
  var left = x + mw > ww ? ww - mw - 8 : x;
  var top = y + mh > wh ? wh - mh - 8 : y;
  if (left < 8) left = 8;
  if (top < 8) top = 8;
  menu.style.left = left + 'px';
  menu.style.top = top + 'px';
  _msgMenu = menu;

  menu.addEventListener('click', function (e) {
    var btn = e.target.closest('[data-action]');
    if (!btn) return;
    var action = btn.dataset.action;
    closeMsgMenu();
    if (action === 'pin') doTogglePin(msg);
    else if (action === 'quote') doQuote(msg);
    else if (action === 'forward') doForward(msg);
  });
}

document.addEventListener('click', function () { closeMsgMenu(); });

function bindMsgContextMenu(container) {
  container.addEventListener('contextmenu', function (e) {
    var row = e.target.closest('.msg-row');
    if (!row) return;
    e.preventDefault();
    showMsgMenu(row, e.clientX, e.clientY);
  });
  container.addEventListener('touchstart', function (e) {
    var row = e.target.closest('.msg-row');
    if (!row) return;
    if (e.target.closest('a, button, img')) return;
    _longPressTimer = setTimeout(function () {
      _longPressTimer = null;
      var touch = e.touches[0];
      if (touch) showMsgMenu(row, touch.clientX, touch.clientY);
    }, 500);
  }, { passive: true });
  container.addEventListener('touchend', function () {
    if (_longPressTimer) { clearTimeout(_longPressTimer); _longPressTimer = null; }
  });
  container.addEventListener('touchmove', function () {
    if (_longPressTimer) { clearTimeout(_longPressTimer); _longPressTimer = null; }
  });
}

async function doTogglePin(msg) {
  var newPinned = !msg.is_pinned;
  try {
    if (state.activeKind === 'room') {
      await Tapp.federation.pinRoomMessage(state.activeId, msg.message_id, newPinned);
    }
    msg.is_pinned = newPinned;
    state.messagesFp = messagesFingerprint(state.messages);
    state.pinnedBarDismissed = false;
    renderMessages();
  } catch (e) {
    try { Tapp.ui.showNotification({ title: lang.pinFail || 'Pin failed', type: 'error' }); } catch (e2) { /* ignore */ }
  }
}

function doQuote(msg) {
  var sender = (msg.sender_actor || '').split('/').pop() || '?';
  var text = getPayloadText(msg.payload) || '';
  if (!text && msg.payload) text = msg.payload.title || msg.payload.filename || '';
  state.quoteMsg = { message_id: msg.message_id, sender: sender, text: text };
  renderQuotePreview();
  var input = $('msg-input');
  if (input) input.focus();
}

function clearQuote() {
  state.quoteMsg = null;
  renderQuotePreview();
}

function renderQuotePreview() {
  var wrap = $('quote-preview');
  if (!wrap) return;
  if (!state.quoteMsg) { wrap.style.display = 'none'; wrap.innerHTML = ''; return; }
  wrap.style.display = 'flex';
  wrap.innerHTML =
    '<div class="quote-preview-bar"></div>'
    + '<div class="quote-preview-body">'
    + '<div class="quote-preview-sender">' + esc(state.quoteMsg.sender) + '</div>'
    + '<div class="quote-preview-text">' + esc(state.quoteMsg.text) + '</div>'
    + '</div>'
    + '<button class="quote-preview-close" id="quote-close">&times;</button>';
  var closeBtn = $('quote-close');
  if (closeBtn) closeBtn.addEventListener('click', clearQuote);
}

function doForward(msg) {
  var items = [];
  state.channels.forEach(function (ch) {
    items.push({
      kind: 'channel',
      id: ch.channel_id,
      name: ch.remote_actor_name || (ch.remote_actor_url || '').split('/').pop() || '?',
      avatar: ch.remote_actor_avatar || '',
    });
  });
  state.rooms.forEach(function (rm) {
    items.push({
      kind: 'room',
      id: rm.room_id,
      name: rm.name || '?',
      avatar: rm.avatar_url || '',
    });
  });
  items = items.filter(function (it) { return it.id !== state.activeId; });
  if (items.length === 0) return;

  var overlay = document.createElement('div');
  overlay.className = 'forward-overlay';
  overlay.innerHTML =
    '<div class="forward-sheet">'
    + '<div class="forward-header">'
    + '<div class="forward-title">' + esc(lang.forwardTo) + '</div>'
    + '<button class="forward-close">&times;</button>'
    + '</div>'
    + '<div class="forward-list"></div>'
    + '</div>';
  var listEl = overlay.querySelector('.forward-list');
  items.forEach(function (it) {
    var btn = document.createElement('button');
    btn.className = 'forward-item';
    btn.innerHTML = '<div class="forward-item-avatar">' + avatarContentHtml(it.avatar || '', it.name) + '</div><span>' + esc(it.name) + '</span>';
    btn.addEventListener('click', async function () {
      overlay.remove();
      var payload = msg.payload;
      var msgType = msg.message_type || 'text';
      try {
        if (it.kind === 'channel') {
          await Tapp.federation.sendMessage(it.id, { payload: payload, message_type: msgType });
        } else {
          await Tapp.federation.sendRoomMessage(it.id, { payload: payload, message_type: msgType });
        }
        try { Tapp.ui.showNotification({ title: lang.forwardSuccess, type: 'success' }); } catch (e2) {}
      } catch (e) {
        notifyError(lang.sendFail, e);
      }
    });
    listEl.appendChild(btn);
  });
  overlay.querySelector('.forward-close').addEventListener('click', function () { overlay.remove(); });
  overlay.addEventListener('click', function (e) { if (e.target === overlay) overlay.remove(); });
  document.body.appendChild(overlay);
}

// ==================== Render: Messages ====================
function renderMessages() {
  var container = $('messages');
  if (!container) return;
  state.pinnedBarDismissed = false;

  if (state.messages.length === 0) {
    var hint = state.activeKind === 'channel' ? lang.emptyChatHint : lang.emptyRoomHint;
    container.innerHTML = '<div class="messages-empty"><div class="messages-empty-icon">'
      + (state.activeKind === 'channel' ? SVG_ICONS.channel : SVG_ICONS.room)
      + '</div><p>' + esc(hint) + '</p></div>';
    var pb = $('pinned-bar'); if (pb) pb.style.display = 'none';
    return;
  }

  var html = '';
  var lastDayKey = '';
  state.messages.forEach(function (msg, idx) {
    var local = isLocalActor(msg.sender_actor);
    var sender = (msg.sender_actor || '').split('/').pop() || '?';
    var payload = (typeof msg.payload === 'object' && msg.payload) ? msg.payload : {};
    var msgType = msg.message_type || 'text';
    // Auto-detect content type from payload when message_type is generic
    if (msgType === 'text' || !msgType) {
      if (payload.content_type && typeof payload.content_type === 'string') {
        msgType = payload.content_type;
      } else if (payload.tapp_id) {
        msgType = 'tapp';
      } else if (payload.brew_id || payload.brew_link) {
        msgType = 'brew';
      } else if (payload.report_id) {
        msgType = 'report';
      } else if (payload.data && payload.mime_type && payload.mime_type.indexOf('image/') === 0) {
        msgType = 'image';
      } else if (payload.data && payload.filename) {
        msgType = 'file';
      }
    }
    var text = getPayloadText(msg.payload);
    var pinned = msg.is_pinned ? '<span class="msg-pin"><svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M9 11V4a1 1 0 011-1h4a1 1 0 011 1v7"/><path d="M5 17h14"/><path d="M7 11l-2 6h14l-2-6"/></svg></span>' : '';

    // Resolve avatar and display name for remote messages
    var avatarUrl = '';
    var displayName = sender;
    if (!local) {
      if (state.activeKind === 'channel' && state.channelDetail) {
        avatarUrl = state.channelDetail.remote_actor_avatar || '';
        displayName = state.channelDetail.remote_actor_name || sender;
      } else if (state.activeKind === 'room') {
        var member = findMemberByActor(msg.sender_actor);
        if (member) {
          displayName = member.display_name || sender;
          avatarUrl = member.avatar_url || '';
        }
      }
    }

    // Day separators
    var dayKey = '';
    try {
      var md = new Date(msg.created_at);
      if (!isNaN(md)) dayKey = md.getFullYear() + '-' + md.getMonth() + '-' + md.getDate();
    } catch (e) { dayKey = ''; }
    if (dayKey && dayKey !== lastDayKey) {
      lastDayKey = dayKey;
      html += '<div class="msg-day-sep"><span class="msg-day-label">' + esc(dayLabel(msg.created_at)) + '</span></div>';
    }

    // Compact: same sender within ~5 minutes
    var prevMsg = idx > 0 ? state.messages[idx - 1] : null;
    var sameSender = prevMsg && sameActorUrl(prevMsg.sender_actor, msg.sender_actor);
    var compact = false;
    if (sameSender && prevMsg && prevMsg.created_at && msg.created_at) {
      try {
        var dt = Math.abs(new Date(msg.created_at) - new Date(prevMsg.created_at));
        compact = dt < 5 * 60 * 1000;
      } catch (e2) { compact = false; }
    }

    html += '<div class="msg-row ' + (local ? 'msg-local' : 'msg-remote') + (compact ? ' msg-compact' : '') + '" data-msg-id="' + esc(msg.message_id || '') + '">';
    if (!local) {
      if (compact) {
        html += '<div class="msg-avatar-spacer"></div>';
      } else {
        html += '<div class="msg-avatar">' + avatarContentHtml(avatarUrl, displayName) + '</div>';
      }
    }
    html += '<div class="msg-bubble ' + (local ? 'bubble-local' : 'bubble-remote') + '">';
    html += '<button type="button" class="msg-more-btn" title="' + esc(lang.msgActions || 'Message actions') + '" aria-label="' + esc(lang.msgActions || 'Message actions') + '">'
      + '<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="19" cy="12" r="1.6"/></svg>'
      + '</button>';
    if (!local && !compact) {
      html += '<div class="msg-sender">' + esc(displayName) + '</div>';
    }

    // Render quoted message if present
    if (payload.quote_sender || payload.quote_text) {
      html += '<div class="msg-quote-block">'
        + '<div class="msg-quote-bar"></div>'
        + '<div class="msg-quote-content">'
        + '<div class="msg-quote-sender">' + esc(payload.quote_sender || '') + '</div>'
        + '<div class="msg-quote-text">' + esc(payload.quote_text || '') + '</div>'
        + '</div></div>';
    }

    // Render content based on message type
    if (msgType === 'image' && payload.data) {
      html += '<img class="msg-image" src="' + esc(payload.data) + '" alt="' + esc(payload.filename || '') + '" />';
      if (payload.text) html += '<div class="msg-text">' + esc(payload.text) + '</div>';
    } else if (msgType === 'file') {
      var ext = (payload.filename || '').split('.').pop().toUpperCase();
      html += '<div class="msg-file-card">'
        + '<div class="msg-file-icon">' + SVG_ICONS.file + '</div>'
        + '<div class="msg-file-info">'
        + '<div class="msg-file-name">' + esc(payload.filename || 'file') + '</div>'
        + '<div class="msg-file-size">' + (payload.size ? formatFileSize(payload.size) : ext) + '</div>'
        + '</div></div>';
      if (payload.text) html += '<div class="msg-text">' + esc(payload.text) + '</div>';
    } else if (msgType === 'tapp' || msgType === 'brew' || msgType === 'library' || msgType === 'report') {
      var shareIcons = { tapp: SVG_ICONS.tapp, brew: SVG_ICONS.brew, library: SVG_ICONS.library, report: SVG_ICONS.report };
      var shareBgs = { tapp: 'rgba(var(--tapp-primary-rgb,100,100,255),.15)', brew: 'rgba(34,197,94,.1)', library: 'rgba(168,85,247,.1)', report: 'rgba(239,68,68,.1)' };
      var shareCardId = 'share-card-' + idx;
      // Determine icon content: use tapp_icon SVG if available, else emoji
      var iconContent = '';
      if (msgType === 'tapp' && payload.tapp_icon) {
        iconContent = payload.tapp_icon; // raw SVG string
      } else {
        iconContent = payload.icon || shareIcons[msgType] || SVG_ICONS.file;
      }
      // Determine tapp share acceptance status from storage
      var tappAcceptStatus = '';
      if (msgType === 'tapp' && payload.tapp_id) {
        var stKey = 'tapp_accept_' + payload.tapp_id + '_' + idx;
        tappAcceptStatus = (state.tappAcceptMap && state.tappAcceptMap[stKey]) || '';
      }
      html += '<div class="msg-share-card" id="' + shareCardId + '"'
        + ' style="cursor:pointer" data-type="' + esc(msgType) + '"'
        + (payload.tapp_id ? ' data-tapp-id="' + esc(payload.tapp_id) + '"' : '')
        + (payload.tapp_version ? ' data-tapp-version="' + esc(payload.tapp_version) + '"' : '')
        + (payload.tapp_name ? ' data-tapp-name="' + esc(payload.tapp_name) + '"' : '')
        + (payload.brew_id ? ' data-brew-id="' + esc(String(payload.brew_id)) + '"' : '')
        + (payload.brew_link ? ' data-brew-link="' + esc(payload.brew_link) + '"' : '')
        + (payload.report_id ? ' data-report-id="' + esc(payload.report_id) + '"' : '')
        + ' data-msg-idx="' + idx + '"'
        + '>'
        + '<div class="msg-share-icon" style="background:' + (shareBgs[msgType] || '') + '">' + iconContent + '</div>'
        + '<div class="msg-share-body">'
        + '<div class="msg-share-type">' + esc(shareTypeLabel(msgType)) + '</div>'
        + '<div class="msg-share-title">' + esc(payload.title || '') + '</div>'
        + (payload.description ? '<div class="msg-share-desc">' + esc(payload.description) + '</div>' : '');
      // Version badge + status pill for tapp
      if (msgType === 'tapp') {
        html += '<div class="msg-share-meta">';
        if (payload.tapp_version) html += '<span class="msg-share-ver">v' + esc(payload.tapp_version) + '</span>';
        if (local) {
          // Sender: show pending status
          html += '<span class="msg-share-status msg-share-status-pending">' + esc(lang.tappSharePending) + '</span>';
        } else if (tappAcceptStatus === 'accepted') {
          html += '<span class="msg-share-status msg-share-status-accepted">' + esc(lang.tappShareAccepted) + '</span>';
        } else if (tappAcceptStatus === 'rejected') {
          html += '<span class="msg-share-status msg-share-status-rejected">' + esc(lang.tappShareRejected) + '</span>';
        }
        html += '</div>';
        // Receiver: show accept/reject buttons if not yet decided
        if (!local && !tappAcceptStatus) {
          html += '<div class="msg-share-actions">'
            + '<button class="msg-share-btn-accept" data-accept-idx="' + idx + '">' + esc(lang.acceptTapp) + '</button>'
            + '<button class="msg-share-btn-reject" data-reject-idx="' + idx + '">' + esc(lang.rejectTapp) + '</button>'
            + '</div>';
        }
      }
      html += '</div></div>';
      if (payload.text) html += '<div class="msg-text">' + esc(payload.text) + '</div>';
    } else {
      html += '<div class="msg-text">' + esc(text) + '</div>';
    }

    html += '<div class="msg-footer">' + pinned + '<span class="msg-time" title="' + esc(fullTimeStr(msg.created_at)) + '">' + timeStr(msg.created_at) + '</span></div>'
      + '</div></div>';
  });
  container.innerHTML = html;
  container.scrollTop = container.scrollHeight;

  // Hover ⋯ opens message menu (desktop); long-press still handled in bindMsgContextMenu
  container.querySelectorAll('.msg-more-btn').forEach(function (btn) {
    btn.addEventListener('click', function (e) {
      e.preventDefault();
      e.stopPropagation();
      var row = btn.closest('.msg-row');
      if (!row) return;
      var rect = btn.getBoundingClientRect();
      showMsgMenu(row, rect.left, rect.bottom + 4);
    });
  });

  // Bind tapp accept/reject buttons
  container.querySelectorAll('.msg-share-btn-accept').forEach(function (btn) {
    btn.addEventListener('click', function (e) {
      e.stopPropagation();
      var msgIdx = btn.dataset.acceptIdx;
      var card = btn.closest('.msg-share-card');
      var tappId = card ? card.dataset.tappId : '';
      if (!tappId) return;
      var stKey = 'tapp_accept_' + tappId + '_' + msgIdx;
      if (!state.tappAcceptMap) state.tappAcceptMap = {};
      state.tappAcceptMap[stKey] = 'accepted';
      Tapp.storage.set(stKey, 'accepted').catch(function () {});
      // Open install detail immediately
      openTappDetail(tappId, card);
      renderMessages();
    });
  });
  container.querySelectorAll('.msg-share-btn-reject').forEach(function (btn) {
    btn.addEventListener('click', function (e) {
      e.stopPropagation();
      var msgIdx = btn.dataset.rejectIdx;
      var card = btn.closest('.msg-share-card');
      var tappId = card ? card.dataset.tappId : '';
      if (!tappId) return;
      var stKey = 'tapp_accept_' + tappId + '_' + msgIdx;
      if (!state.tappAcceptMap) state.tappAcceptMap = {};
      state.tappAcceptMap[stKey] = 'rejected';
      Tapp.storage.set(stKey, 'rejected').catch(function () {});
      renderMessages();
    });
  });
  // Bind share card click handlers — open detail views
  container.querySelectorAll('.msg-share-card[data-type]').forEach(function (card) {
    card.addEventListener('click', function (e) {
      // Don't open detail if clicking on action buttons
      if (e.target.closest('.msg-share-actions')) return;
      var type = card.dataset.type;
      if (type === 'tapp' && card.dataset.tappId) {
        // Only open detail if accepted or if sender
        var msgIdx = card.dataset.msgIdx;
        var stKey = 'tapp_accept_' + card.dataset.tappId + '_' + msgIdx;
        var status = state.tappAcceptMap && state.tappAcceptMap[stKey];
        var isLocal = card.closest('.msg-local');
        if (isLocal || status === 'accepted') {
          openTappDetail(card.dataset.tappId, card);
        }
      } else if (type === 'brew' && card.dataset.brewId) {
        openBrewDetail(parseInt(card.dataset.brewId), card.dataset.brewLink, card);
      } else if (type === 'report' && card.dataset.reportId) {
        openReportDetail(card.dataset.reportId, card);
      }
    });
  });
  renderPinnedBar();
  bindMsgContextMenu(container);
}

/* ----- Shared detail overlay for received content ----- */
function createDetailOverlay(title, iconHtml, bgColor) {
  var overlay = document.createElement('div');
  overlay.className = 'picker-overlay';
  overlay.innerHTML =
    '<div class="picker-sheet">'
    + '<div class="picker-header">'
    + '<div class="picker-header-icon" style="background:' + esc(bgColor) + '">' + iconHtml + '</div>'
    + '<div class="picker-header-title">' + esc(title) + '</div>'
    + '<button class="picker-close-btn">&times;</button>'
    + '</div>'
    + '<div class="picker-body"></div>'
    + '</div>';
  overlay.querySelector('.picker-close-btn').addEventListener('click', function () { overlay.remove(); });
  overlay.addEventListener('click', function (e) { if (e.target === overlay) overlay.remove(); });
  document.body.appendChild(overlay);
  return overlay;
}

function openTappDetail(tappId, card) {
  // Extract sender-provided info from the share card / data attributes
  var remoteName = (card.querySelector('.msg-share-title') || {}).textContent || card.dataset.tappName || tappId;
  var remoteDesc = (card.querySelector('.msg-share-desc') || {}).textContent || '';
  var remoteVersion = card.dataset.tappVersion || '';

  var overlay = createDetailOverlay(remoteName, SVG_ICONS.tapp, 'rgba(var(--tapp-primary-rgb,100,100,255),.1)');
  var body = overlay.querySelector('.picker-body');
  showPickerLoading(body);

  // Check local installation
  Tapp.tappList.get(tappId).then(function (local) {
    var installed = local && local.status && local.status !== 'uninstalled';
    var localVer = installed ? (local.version || '') : '';
    var needsUpdate = installed && remoteVersion && localVer && localVer !== remoteVersion;
    renderTappDetailView(body, tappId, remoteName, remoteDesc, remoteVersion, installed, localVer, needsUpdate);
  }).catch(function () {
    // Can't determine local status — assume not installed
    renderTappDetailView(body, tappId, remoteName, remoteDesc, remoteVersion, false, '', false);
  });
}

function renderTappDetailView(body, tappId, name, desc, remoteVer, installed, localVer, needsUpdate) {
  var statusColor = installed ? (needsUpdate ? '#f59e0b' : '#22c55e') : '#ef4444';
  var statusText = installed ? (needsUpdate ? lang.tappUpdateAvail : lang.tappInstalled) : lang.tappNotInstalled;
  var statusIcon = installed ? (needsUpdate ? '⚠️' : '✅') : '❌';

  var html = '<div style="padding:16px;display:flex;flex-direction:column;gap:14px">'
    + '<div style="font-size:18px;font-weight:700">' + esc(name) + '</div>'
    + '<div style="font-size:12px;color:var(--text-secondary,#888)">' + esc(tappId) + '</div>'
    + (desc ? '<div style="font-size:13px;line-height:1.6">' + esc(desc) + '</div>' : '')
    // Version comparison
    + '<div style="display:flex;flex-direction:column;gap:6px;padding:12px;border-radius:10px;background:rgba(128,128,128,.06)">'
    + '<div style="display:flex;align-items:center;gap:8px">'
    + '<span style="font-size:14px">' + statusIcon + '</span>'
    + '<span style="font-size:13px;font-weight:600;color:' + statusColor + '">' + esc(statusText) + '</span>'
    + '</div>';

  if (remoteVer) {
    html += '<div style="font-size:12px;color:var(--text-secondary,#888)">' + esc(lang.remoteVer) + ': v' + esc(remoteVer) + '</div>';
  }
  if (localVer) {
    html += '<div style="font-size:12px;color:var(--text-secondary,#888)">' + esc(lang.localVer) + ': v' + esc(localVer) + '</div>';
  }
  html += '</div>';

  // Action button
  if (!installed) {
    html += '<button class="tapp-action-btn" data-action="install" style="width:100%;padding:12px;border:none;border-radius:10px;font-size:14px;font-weight:600;cursor:pointer;background:var(--tapp-primary,#6366f1);color:#fff">' + esc(lang.installBtn) + '</button>';
  } else if (needsUpdate) {
    html += '<button class="tapp-action-btn" data-action="update" style="width:100%;padding:12px;border:none;border-radius:10px;font-size:14px;font-weight:600;cursor:pointer;background:#f59e0b;color:#fff">' + esc(lang.updatingBtn) + '</button>';
  } else {
    html += '<div style="text-align:center;font-size:12px;color:var(--text-secondary,#888)">' + esc(lang.alreadyLatest) + '</div>';
  }

  html += '</div>';
  body.innerHTML = html;

  // Bind install/update button
  var actionBtn = body.querySelector('.tapp-action-btn');
  if (actionBtn) {
    actionBtn.addEventListener('click', function handleInstallClick() {
      if (actionBtn.disabled) return;
      actionBtn.disabled = true;
      actionBtn.textContent = lang.installingBtn;
      actionBtn.style.opacity = '0.7';

      Tapp.tappList.install({ source: 'store', tappId: tappId }).then(function (result) {
        actionBtn.textContent = lang.installSuccess;
        actionBtn.style.background = '#22c55e';
        actionBtn.style.opacity = '1';
        actionBtn.removeEventListener('click', handleInstallClick);
      }).catch(function () {
        actionBtn.textContent = lang.installFailed;
        actionBtn.style.background = '#ef4444';
        actionBtn.style.opacity = '1';
        actionBtn.disabled = false;
      });
    });
  }
}

function openBrewDetail(brewId, brewLink, card) {
  var overlay = createDetailOverlay(card.querySelector('.msg-share-title').textContent || 'Brew', SVG_ICONS.brew, 'rgba(34,197,94,.1)');
  var body = overlay.querySelector('.picker-body');
  showPickerLoading(body);
  Tapp.brewList.get(brewId).then(function (detail) {
    if (!detail) { body.innerHTML = '<div class="picker-empty">' + esc(lang.pickerEmpty) + '</div>'; return; }
    body.innerHTML =
      '<div style="padding:16px;display:flex;flex-direction:column;gap:12px">'
      + (detail.image ? '<img src="' + esc(detail.image) + '" style="width:100%;max-height:200px;object-fit:cover;border-radius:8px" />' : '')
      + '<div style="font-size:18px;font-weight:600">' + esc(detail.title) + '</div>'
      + '<div style="font-size:12px;color:var(--text-secondary,#888)">' + esc((detail.source_name || '') + (detail.author ? ' · ' + detail.author : '') + (detail.published_at ? ' · ' + new Date(detail.published_at).toLocaleDateString() : '')) + '</div>'
      + (detail.summary ? '<div style="font-size:13px;line-height:1.6">' + esc(detail.summary) + '</div>' : '')
      + (brewLink ? '<a href="' + esc(brewLink) + '" target="_blank" rel="noopener noreferrer" style="font-size:12px;color:var(--tapp-primary,#6366f1);text-decoration:none">' + esc(lang.openOriginal || 'Open original') + ' →</a>' : '')
      + '</div>';
  }).catch(function () {
    body.innerHTML = '<div class="picker-empty">' + esc(lang.pickerEmpty) + '</div>';
  });
}

function openReportDetail(reportId, card) {
  var overlay = createDetailOverlay(card.querySelector('.msg-share-title').textContent || 'Report', SVG_ICONS.report, 'rgba(239,68,68,.1)');
  var body = overlay.querySelector('.picker-body');
  showPickerLoading(body);
  Tapp.report.getReport(reportId).then(function (detail) {
    if (!detail) { body.innerHTML = '<div class="picker-empty">' + esc(lang.pickerEmpty) + '</div>'; return; }
    body.innerHTML =
      '<div style="padding:16px;display:flex;flex-direction:column;gap:12px">'
      + '<div style="font-size:18px;font-weight:600">' + esc(detail.summary || detail.type || 'Report') + '</div>'
      + '<div style="font-size:12px;color:var(--text-secondary,#888)">' + esc((detail.platform || '') + (detail.type ? ' · ' + detail.type : '') + (detail.createdAt ? ' · ' + new Date(detail.createdAt).toLocaleDateString() : '')) + '</div>'
      + (detail.content ? '<div style="font-size:13px;line-height:1.6;max-height:300px;overflow-y:auto">' + esc(detail.content) + '</div>' : '')
      + '</div>';
  }).catch(function () {
    body.innerHTML = '<div class="picker-empty">' + esc(lang.pickerEmpty) + '</div>';
  });
}

// ==================== Render: Members ====================
function renderMembers() {
  var panel = $('member-panel');
  if (!panel) return;

  if (state.activeKind !== 'room' || !state.roomDetail) {
    panel.style.display = 'none';
    return;
  }
`

const PAGE_MOD_MEMBERS = `\
  panel.style.display = '';
  $('member-title').textContent = lang.members + ' (' + state.members.length + ')';

  var myRole = state.roomDetail.my_role || '';
  var canKick = (myRole === 'owner' || myRole === 'admin');

  var html = '';
  state.members.forEach(function (m) {
    var name = m.display_name || (m.actor_url || '').split('/').pop() || '?';
    // 普通成员不显示角色，减少列表噪音；仅标出群主/管理员
    var roleText = (m.role && m.role !== 'member') ? roleLabel(m.role) : '';
    html += '<div class="member-item">'
      + '<div class="member-avatar">' + avatarContentHtml(m.avatar_url || '', name) + '</div>'
      + '<div class="member-info">'
      + '<div class="member-name">' + esc(name) + '</div>'
      + (roleText ? '<div class="member-role">' + esc(roleText) + '</div>' : '')
      + '</div>';
    if (m.is_local) {
      html += '<span class="member-local">' + esc(lang.local) + '</span>';
    } else if (canKick && m.role !== 'owner') {
      html += '<button class="member-kick" data-actor="' + esc(m.actor_url || '') + '" title="' + esc(lang.kick) + '">'
        + '<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>'
        + '</button>';
    }
    html += '</div>';
  });
  $('member-list').innerHTML = html;

  // Wire kick buttons
  if (canKick) {
    var kicks = document.querySelectorAll('.member-kick');
    kicks.forEach(function (btn) {
      btn.addEventListener('click', function () {
        var actor = btn.getAttribute('data-actor');
        if (actor) doKickMember(actor);
      });
    });
  }

  // Show invite icon for any room member
  var inviteWrap = $('invite-wrap');
  if (inviteWrap) {
    inviteWrap.style.display = (state.roomDetail && myRole) ? '' : 'none';
  }
}

// ==================== Manage Dropdown ====================
function toggleManageDropdown(e) {
  e && e.stopPropagation();
  var dd = $('manage-dropdown');
  if (!dd) return;
  dd.classList.toggle('open');
}
function closeManageDropdown() {
  var dd = $('manage-dropdown');
  if (dd) dd.classList.remove('open');
}
document.addEventListener('click', function (e) {
  var dd = $('manage-dropdown');
  if (!dd || !dd.classList.contains('open')) return;
  var wrap = dd.parentElement;
  if (wrap && !wrap.contains(e.target)) closeManageDropdown();
});

// ==================== Render: Chat Header ====================
function renderChatHeader() {
  var nameEl = $('chat-name');
  var metaEl = $('chat-meta');
  var actionsEl = $('chat-actions');
  var avatarEl = $('chat-hdr-avatar');
  if (!nameEl) return;

  if (state.activeKind === 'channel' && state.channelDetail) {
    var ch = state.channelDetail;
    var chName = ch.remote_actor_name || (ch.remote_actor_url || '').split('/').pop() || '?';
    nameEl.textContent = chName;
    if (avatarEl) {
      avatarEl.innerHTML = avatarContentHtml(ch.remote_actor_avatar || '', chName);
    }
    metaEl.innerHTML = '<span class="meta-badge badge-channel">' + esc(lang.dm) + '</span>'
      + (ch.status === 'pending' ? '<span class="meta-badge badge-pending">' + esc(lang.pending) + '</span>' : '');
    var actionsHtml = '';
    if (ch.status === 'pending' && ch.initiated_by === 'remote') {
      actionsHtml += '<button class="action-btn action-accept" id="action-accept">' + esc(lang.accept) + '</button>';
    }
    if (ch.status !== 'closed') {
      actionsHtml += '<div class="manage-wrap"><button class="manage-btn" id="manage-toggle" title="' + esc(lang.manage) + '">⋯</button>'
        + '<div class="manage-dropdown" id="manage-dropdown">'
        + '<button class="manage-item manage-item-danger" id="action-close">'
        + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>'
        + esc(lang.close) + '</button></div></div>';
    } else {
      actionsHtml += '<span class="meta-badge badge-closed">' + esc(lang.closed) + '</span>';
    }
    actionsEl.innerHTML = actionsHtml;
  } else if (state.activeKind === 'room' && state.roomDetail) {
    var rm = state.roomDetail;
    nameEl.textContent = rm.name || '?';
    if (avatarEl) {
      avatarEl.innerHTML = avatarContentHtml(rm.avatar_url || '', rm.name || '?');
    }
    metaEl.innerHTML = '<span class="meta-badge badge-room">' + (rm.member_count || 0) + ' ' + esc(lang.members) + '</span>'
      + (rm.my_role && rm.my_role !== 'member' ? '<span class="meta-badge badge-role">' + esc(roleLabel(rm.my_role)) + '</span>' : '');
    var menuItems = '';
    if (rm.my_role === 'owner' || rm.my_role === 'admin') {
      menuItems += '<button class="manage-item" id="action-edit-room">'
        + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>'
        + esc(lang.editRoom) + '</button>';
    }
    if (rm.my_role !== 'owner') {
      menuItems += '<button class="manage-item manage-item-danger" id="action-leave">'
        + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4M16 17l5-5-5-5M21 12H9"/></svg>'
        + esc(lang.leave) + '</button>';
    }
    if (rm.my_role === 'owner') {
      menuItems += '<button class="manage-item manage-item-danger" id="action-dissolve">'
        + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18M8 6V4h8v2M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6M10 11v6M14 11v6"/></svg>'
        + esc(lang.dissolve) + '</button>';
    }
    // Member toggle button + manage menu
    var memberToggleHtml = '<button class="member-toggle-btn" id="member-toggle-btn" title="' + esc(lang.members) + '">'
      + '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 00-3-3.87M16 3.13a4 4 0 010 7.75"/></svg>'
      + '</button>';
    if (menuItems) {
      actionsEl.innerHTML = memberToggleHtml + '<div class="manage-wrap"><button class="manage-btn" id="manage-toggle" title="' + esc(lang.manage) + '">⋯</button>'
        + '<div class="manage-dropdown" id="manage-dropdown">' + menuItems + '</div></div>';
    } else {
      actionsEl.innerHTML = memberToggleHtml;
    }
  }

  var acceptBtn = $('action-accept');
  if (acceptBtn) acceptBtn.addEventListener('click', doAcceptChannel);
  var closeBtn = $('action-close');
  if (closeBtn) closeBtn.addEventListener('click', function () { closeManageDropdown(); doCloseChannel(); });
  var leaveBtn = $('action-leave');
  if (leaveBtn) leaveBtn.addEventListener('click', function () { closeManageDropdown(); doLeaveRoom(); });
  var editRoomBtn = $('action-edit-room');
  if (editRoomBtn) editRoomBtn.addEventListener('click', function () { closeManageDropdown(); showEditRoomDialog(); });
  var dissolveBtn = $('action-dissolve');
  if (dissolveBtn) dissolveBtn.addEventListener('click', function () { closeManageDropdown(); doDissolveRoom(); });
  var toggleBtn = $('manage-toggle');
  if (toggleBtn) toggleBtn.addEventListener('click', toggleManageDropdown);
  var memberToggle = $('member-toggle-btn');
  if (memberToggle) memberToggle.addEventListener('click', toggleMemberPanel);
}

// ==================== Member Panel Toggle ====================
state.memberPanelOpen = true; // default open on desktop

function isTablet() { var w = window.innerWidth; return w >= 769 && w <= 1024; }

function toggleMemberPanel() {
  var panel = $('member-panel');
  if (!panel) return;
  var isMobile = window.innerWidth <= 768;
  if (isMobile) {
    panel.classList.toggle('member-open-mobile');
  } else if (isTablet()) {
    panel.classList.toggle('member-expanded-tablet');
    state.memberPanelOpen = panel.classList.contains('member-expanded-tablet');
  } else {
    panel.classList.toggle('member-collapsed');
    state.memberPanelOpen = !panel.classList.contains('member-collapsed');
  }
}

function closeMemberPanel() {
  var panel = $('member-panel');
  if (!panel) return;
  panel.classList.remove('member-open-mobile');
  panel.classList.remove('member-expanded-tablet');
  if (window.innerWidth > 768 && !isTablet()) {
    panel.classList.add('member-collapsed');
  }
  state.memberPanelOpen = false;
}

// ==================== API ====================
async function loadConversations() {
  try {
    var results = await Promise.allSettled([
      Tapp.federation.getChannels(),
      Tapp.federation.getRooms(),
    ]);
    var errors = [];
    if (results[0].status === 'fulfilled' && results[0].value) {
      state.channels = results[0].value.channels || [];
`

const PAGE_MOD_API = `\
    } else if (results[0].status === 'rejected') {
      console.error('[Aro] getChannels failed:', results[0].reason);
      errors.push(String(results[0].reason));
    }
    if (results[1].status === 'fulfilled' && results[1].value) {
      state.rooms = results[1].value.rooms || [];
    } else if (results[1].status === 'rejected') {
      console.error('[Aro] getRooms failed:', results[1].reason);
      errors.push(String(results[1].reason));
    }
    renderConvList();
    if (errors.length > 0 && state.channels.length === 0 && state.rooms.length === 0) {
      var list = $('conv-list');
      if (list) list.innerHTML = '<div class="conv-empty conv-empty-fill" style="color:#ef4444;font-size:12px;white-space:pre-wrap">' + esc(errors.join('\\n')) + '</div>';
    }
  } catch (e) {
    console.error('[Aro] loadConversations error:', e);
  }
}

async function openConversation(kind, id) {
  // Drop previous realtime subscription before switching
  await unsubscribeRealtime();

  state.activeKind = kind;
  state.activeId = id;
  state.messages = [];
  state.messagesFp = '';
  state.members = [];
  state.channelDetail = null;
  state.roomDetail = null;

  $('empty-state').style.display = 'none';
  $('chat-container').style.display = '';
  $('sidebar').classList.add('sidebar-hidden-mobile');

  renderMessages();
  renderChatHeader();

  try {
    if (kind === 'channel') {
      var results = await Promise.all([
        Tapp.federation.getChannel(id),
        Tapp.federation.getMessages(id, undefined, 100),
      ]);
      if (results[0]) {
        state.channelDetail = results[0];
        // Derive local actor URL: find a message sender that is NOT the remote actor
        if (!state.localActorUrl && results[0].remote_actor_url) {
          var msgs = (results[1] && results[1].messages) || [];
          for (var mi = 0; mi < msgs.length; mi++) {
            var senderActor = msgs[mi].sender_actor;
            if (senderActor && !sameActorUrl(senderActor, results[0].remote_actor_url)) {
              state.localActorUrl = normalizeFederationUrl(senderActor) || senderActor;
              break;
            }
          }
        }
      }
      if (results[1]) {
        state.messages = results[1].messages || [];
        state.messagesFp = messagesFingerprint(state.messages);
      }
    } else {
      var results = await Promise.all([
        Tapp.federation.getRoom(id),
        Tapp.federation.getRoomMembers(id),
        Tapp.federation.getRoomMessages(id, undefined, 100),
      ]);
      if (results[0]) state.roomDetail = results[0];
      if (results[1]) {
        state.members = unwrapRoomMembers(results[1]);
        // Extract local actor URL from members list
        if (!state.localActorUrl) {
          for (var i = 0; i < state.members.length; i++) {
            var memberActor = normalizeFederationUrl(state.members[i].actor_url);
            if (state.members[i].is_local && memberActor) { state.localActorUrl = memberActor; break; }
          }
        }
      }
      if (results[2]) {
        state.messages = results[2].messages || [];
        state.messagesFp = messagesFingerprint(state.messages);
      }
    }
  } catch (e) {
    console.error('[Aro] openConversation failed:', e);
    notifyError(lang.loadFail || lang.sendFail || 'Load failed', e);
  }

  renderChatHeader();
  renderMessages();
  renderMembers();
  renderConvList();
  updateSendState();
  startPolling();
  subscribeRealtime();
  var focusInput = $('msg-input');
  if (focusInput) {
    try { focusInput.focus(); } catch (e) { /* ignore */ }
  }
}

async function doSend() {
  var input = $('msg-input');
  if (!input) return;

  var text = input.value.trim();
  var attach = state.pendingAttach;

  // Need either text or attachment
  if ((!text && !attach) || !state.activeId || state.sending) return;

  input.value = '';
  autoResizeInput(input);
  updateSendState();
  state.sending = true;
  closeAttachMenu();

  try {
    var msgPayload;
    var msgType;

    if (attach) {
      if (attach.type === 'image') {
        msgType = 'image';
        msgPayload = { data: attach.data, filename: attach.name, mime_type: attach.mime, size: attach.size, text: text || '' };
      } else if (attach.type === 'file') {
        msgType = 'file';
        msgPayload = { data: attach.data, filename: attach.name, mime_type: attach.mime, size: attach.size, text: text || '' };
      } else {
        // Federation content: tapp, brew, library, report
        msgType = attach.type;
        msgPayload = { title: attach.name, description: attach.desc || '', content_type: attach.type, icon: attach.icon || '', text: text || '' };
        // Include resource IDs so the receiver can fetch detail
        if (attach.tappId) msgPayload.tapp_id = attach.tappId;
        if (attach.tappVersion) msgPayload.tapp_version = attach.tappVersion;
        if (attach.tappIcon) msgPayload.tapp_icon = attach.tappIcon;
        if (attach.brewId) msgPayload.brew_id = attach.brewId;
        if (attach.brewLink) msgPayload.brew_link = attach.brewLink;
        if (attach.platformId) msgPayload.platform_id = attach.platformId;
        if (attach.itemId) msgPayload.item_id = attach.itemId;
        if (attach.reportId) msgPayload.report_id = attach.reportId;
      }
      clearPendingAttach();
    } else {
      msgType = 'text';
      msgPayload = { text: text };
    }

    // Attach quote info if replying to a message
    var replyTo = null;
    if (state.quoteMsg) {
      msgPayload.quote_sender = state.quoteMsg.sender;
      msgPayload.quote_text = state.quoteMsg.text;
      msgPayload.quote_id = state.quoteMsg.message_id;
      replyTo = state.quoteMsg.message_id;
      clearQuote();
    }

    var sendReq = { payload: msgPayload, message_type: msgType };
    if (replyTo) sendReq.reply_to = replyTo;
    if (state.activeKind === 'channel') {
      await Tapp.federation.sendMessage(state.activeId, sendReq);
    } else {
      await Tapp.federation.sendRoomMessage(state.activeId, sendReq);
    }
    await pollMessages(true);
  } catch (e) {
    if (text) input.value = text;
    notifyError(lang.sendFail, e);
  } finally {
    state.sending = false;
    updateSendState();
    input.focus();
  }
}

/** Fingerprint message list so pin/content changes refresh even when count stays the same. */
function messagesFingerprint(msgs) {
  if (!msgs || !msgs.length) return '0';
  var last = msgs[msgs.length - 1] || {};
  var pins = 0;
  var ids = [];
  for (var i = 0; i < msgs.length; i++) {
    if (msgs[i].is_pinned) pins++;
    if (i === 0 || i === msgs.length - 1 || msgs[i].is_pinned) {
      ids.push((msgs[i].message_id || '') + (msgs[i].is_pinned ? '*' : ''));
    }
  }
  return msgs.length + '|' + (last.message_id || '') + '|' + (last.created_at || '') + '|' + pins + '|' + ids.join(',');
}

function mergeIncomingMessage(msg) {
  if (!msg || !msg.message_id) return false;
  for (var i = 0; i < state.messages.length; i++) {
    if (state.messages[i].message_id === msg.message_id) {
      state.messages[i] = Object.assign({}, state.messages[i], msg);
      state.messagesFp = messagesFingerprint(state.messages);
      renderMessages();
      return true;
    }
  }
  state.messages.push(msg);
  state.messagesFp = messagesFingerprint(state.messages);
  renderMessages();
  return true;
}

async function pollMessages(force) {
  if (!state.activeId || !state.activeKind) return;
  try {
    var res;
    if (state.activeKind === 'channel') {
      res = await Tapp.federation.getMessages(state.activeId, undefined, 100);
    } else {
      res = await Tapp.federation.getRoomMessages(state.activeId, undefined, 100);
    }
    if (res) {
      var msgs = res.messages || [];
      var fp = messagesFingerprint(msgs);
      if (force || fp !== state.messagesFp) {
        state.messages = msgs;
        state.messagesFp = fp;
        renderMessages();
      }
    }
  } catch (e) { /* ignore */ }
}

function startPolling() {
  stopPolling();
  state.pollTimer = setInterval(function () { pollMessages(false); }, state.pollInterval);
}

function stopPolling() {
  if (state.pollTimer) { clearInterval(state.pollTimer); state.pollTimer = null; }
}

async function subscribeRealtime() {
  if (!state.activeId || !state.activeKind || !Tapp.federation) return;
  // Already subscribed to this conversation
  if (state.subscribedKind === state.activeKind && state.subscribedId === state.activeId) return;
  await unsubscribeRealtime();
  try {
    if (state.activeKind === 'channel' && typeof Tapp.federation.subscribeChannel === 'function') {
      await Tapp.federation.subscribeChannel(state.activeId);
      state.subscribedKind = 'channel';
      state.subscribedId = state.activeId;
    } else if (state.activeKind === 'room' && typeof Tapp.federation.subscribeRoom === 'function') {
      await Tapp.federation.subscribeRoom(state.activeId);
      state.subscribedKind = 'room';
      state.subscribedId = state.activeId;
    }
  } catch (e) {
    console.warn('[Aro] realtime subscribe failed, falling back to poll:', e);
  }
}

async function unsubscribeRealtime() {
  if (!state.subscribedKind || !state.subscribedId || !Tapp.federation) {
    state.subscribedKind = null;
    state.subscribedId = null;
    return;
  }
  try {
    if (state.subscribedKind === 'channel' && typeof Tapp.federation.unsubscribeChannel === 'function') {
      await Tapp.federation.unsubscribeChannel(state.subscribedId);
    } else if (state.subscribedKind === 'room' && typeof Tapp.federation.unsubscribeRoom === 'function') {
      await Tapp.federation.unsubscribeRoom(state.subscribedId);
    }
  } catch (e) { /* ignore */ }
  state.subscribedKind = null;
  state.subscribedId = null;
}

function handleRealtimeMessage(ev) {
  if (!ev) return;
  var data = ev.data || {};
  var scope = ev.scope;
  var scopeId = scope === 'channel' ? ev.channelId : scope === 'room' ? ev.roomId : null;
  var inScope = false;
  if (scope === 'channel' && state.activeKind === 'channel' && ev.channelId === state.activeId) {
    inScope = true;
  } else if (scope === 'room' && state.activeKind === 'room' && ev.roomId === state.activeId) {
    inScope = true;
  }

  // 非当前会话：Toast + 刷新列表（后端通知中心另有 SSE）
  if (!inScope) {
    if (data.type === 'message' && data.message && scopeId) {
      maybeNotifyIncomingMessage(scope, scopeId, data.message);
      loadConversations().catch(function () {});
    }
    return;
  }

  if (data.type === 'message' && data.message) {
    mergeIncomingMessage(data.message);
    // 当前会话但页面在后台时仍提示
    maybeNotifyIncomingMessage(scope, scopeId, data.message);
    return;
  }
  if (data.type === 'room_message_pinned' && data.message_id) {
    for (var i = 0; i < state.messages.length; i++) {
      if (state.messages[i].message_id === data.message_id) {
        state.messages[i].is_pinned = !!data.is_pinned;
        state.messagesFp = messagesFingerprint(state.messages);
        renderMessages();
        return;
      }
    }
    pollMessages(true);
    return;
  }
  if (data.event === 'member_invited' || data.event === 'member_left' || data.event === 'member_kicked' || data.type === 'room_deleted') {
    if (state.activeKind === 'room') {
      Tapp.federation.getRoomMembers(state.activeId).then(function (res) {
        state.members = unwrapRoomMembers(res);
        renderMembers();
        renderChatHeader();
      }).catch(function () {});
      if (data.type === 'room_deleted') {
        notifyError(lang.dissolve || 'Room deleted');
      }
    }
    return;
  }
  // Unknown event — force a full refresh
  pollMessages(true);
}

function bindRealtimeListeners() {
  if (state.realtimeBound || !Tapp.federation) return;
  state.realtimeBound = true;
  if (typeof Tapp.federation.onMessage === 'function') {
    Tapp.federation.onMessage(function (ev) { handleRealtimeMessage(ev); });
  }
  if (typeof Tapp.federation.onChannelUpdate === 'function') {
    Tapp.federation.onChannelUpdate(function (ev) {
      if (!ev || ev.channelId !== state.activeId || state.activeKind !== 'channel') return;
      if (ev.event === 'closed') {
        if (state.channelDetail) state.channelDetail.status = 'closed';
        for (var i = 0; i < state.channels.length; i++) {
          if (state.channels[i].channel_id === state.activeId) {
            state.channels[i].status = 'closed';
            break;
          }
        }
        renderChatHeader();
        renderConvList();
      } else if (ev.event === 'disconnected') {
        // WS dropped — poll will keep things eventually consistent
        pollMessages(true);
      }
    });
  }
  if (typeof Tapp.federation.onRoomUpdate === 'function') {
    Tapp.federation.onRoomUpdate(function (ev) {
      if (!ev || ev.roomId !== state.activeId || state.activeKind !== 'room') return;
      if (ev.event === 'disconnected') pollMessages(true);
      else if (ev.event === 'governance_changed') {
        Tapp.federation.getRoom(state.activeId).then(function (detail) {
          if (detail) { state.roomDetail = detail; renderChatHeader(); }
        }).catch(function () {});
      }
    });
  }
}

async function doCloseChannel() {
  if (!state.activeId || state.activeKind !== 'channel') return;
  if (!(await aroConfirm(lang.closeChannelConfirm, true))) return;
  try {
    await unsubscribeRealtime();
    await Tapp.federation.closeChannel(state.activeId);
    state.channelDetail.status = 'closed';
    renderChatHeader();
    loadConversations();
  } catch (e) { /* ignore */ }
}

async function doInviteMember(actorUrl) {
  if (!state.activeId || state.activeKind !== 'room') return;
  var actor = actorUrl;
  if (!actor) {
    var input = $('invite-input');
    actor = (input ? input.value : '').trim();
  }
  if (!actor) return;
  try {
    await Tapp.federation.inviteMember(state.activeId, { actor: actor });
    if (!actorUrl) { var input2 = $('invite-input'); if (input2) input2.value = ''; }
    try { Tapp.ui.showNotification({ title: lang.inviteSuccess, type: 'success' }); } catch (e2) {}
    // Refresh members & re-render popover
    try {
      var detail = await Tapp.federation.getRoom(state.activeId);
      if (detail) state.roomDetail = detail;
      var membersRes = await Tapp.federation.getRoomMembers(state.activeId);
      state.members = unwrapRoomMembers(membersRes);
      renderMembers();
      renderInvitePopoverContacts();
    } catch (e2) {}
  } catch (e) {
    notifyError(lang.inviteFail, e);
  }
}

// ==================== Invite Popover ====================
// Create popover dynamically on document.body to escape all overflow clipping
var _invitePopover = null;
function ensureInvitePopover() {
  if (_invitePopover) return _invitePopover;
  var div = document.createElement('div');
  div.id = 'invite-popover';
  div.className = 'invite-popover';
  div.style.display = 'none';
  div.innerHTML = '<div class="invite-pop-section">'
    + '<div class="invite-pop-label" id="invite-pop-contacts-label">' + esc(lang.inviteFromContacts) + '</div>'
    + '<div id="invite-pop-list" class="invite-pop-list"></div>'
    + '<div id="invite-pop-empty" class="invite-pop-empty" style="display:none">' + esc(lang.noContacts) + '</div>'
    + '</div>'
    + '<div class="invite-pop-divider"></div>'
    + '<div class="invite-pop-section">'
    + '<div class="invite-pop-label" id="invite-pop-manual-label">' + esc(lang.inviteManual) + '</div>'
    + '<div class="invite-pop-manual">'
    + '<input id="invite-input" class="invite-input" type="text" placeholder="' + esc(lang.invitePlaceholder) + '" />'
    + '<button id="invite-btn" class="invite-pop-send">'
    + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z"/></svg>'
    + '</button>'
    + '</div>'
    + '</div>';
  document.body.appendChild(div);
  // Wire events on the popover elements
  var inviteBtn = div.querySelector('#invite-btn');
  if (inviteBtn) inviteBtn.addEventListener('click', function () { doInviteMember(); });
  var inviteInput = div.querySelector('#invite-input');
  if (inviteInput) inviteInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); doInviteMember(); }
  });
  _invitePopover = div;
  return div;
}

function toggleInvitePopover(e) {
  e && e.stopPropagation();
  var pop = ensureInvitePopover();
  var toggle = $('invite-toggle');
  if (!toggle) return;
  var isOpen = pop.style.display !== 'none';
  if (isOpen) {
    pop.style.display = 'none';
  } else {
    var rect = toggle.getBoundingClientRect();
    pop.style.top = (rect.bottom + 6) + 'px';
    pop.style.left = Math.max(4, rect.right - 240) + 'px';
    pop.style.display = '';
    renderInvitePopoverContacts();
  }
}
function closeInvitePopover() {
  if (_invitePopover) _invitePopover.style.display = 'none';
}
document.addEventListener('click', function (e) {
  if (!_invitePopover || _invitePopover.style.display === 'none') return;
  var wrap = $('invite-wrap');
  if ((wrap && wrap.contains(e.target)) || _invitePopover.contains(e.target)) return;
  closeInvitePopover();
});

function renderInvitePopoverContacts() {
  var listEl = $('invite-pop-list');
  var emptyEl = $('invite-pop-empty');
  if (!listEl || !emptyEl) return;

  // Get actor URLs of current room members for filtering (normalized)
  var memberActors = {};
  state.members.forEach(function (m) {
    if (!m.actor_url) return;
    memberActors[m.actor_url] = true;
    var normalized = normalizeFederationUrl(m.actor_url);
    if (normalized) memberActors[normalized] = true;
  });

  // Build contacts from existing channels (chat partners)
  var contacts = [];
  state.channels.forEach(function (ch) {
    if (!ch.remote_actor_url || ch.status === 'closed') return;
    var remoteNorm = normalizeFederationUrl(ch.remote_actor_url) || ch.remote_actor_url;
    var alreadyMember = !!(memberActors[ch.remote_actor_url] || memberActors[remoteNorm]);
    contacts.push({
      name: ch.remote_actor_name || (ch.remote_actor_url || '').split('/').pop() || '?',
      avatar: ch.remote_actor_avatar || '',
      actorUrl: ch.remote_actor_url,
      alreadyMember: alreadyMember,
    });
  });

  if (contacts.length === 0) {
    listEl.innerHTML = '';
    emptyEl.style.display = '';
    emptyEl.textContent = lang.noContacts;
    return;
  }

  emptyEl.style.display = 'none';
  var html = '';
  contacts.forEach(function (c) {
    var initial = (c.name[0] || '?').toUpperCase();
    var shortUrl = (c.actorUrl || '').replace(/^https?:\\/\\//, '').split('/').slice(0, 2).join('/');
    html += '<button class="invite-pop-contact' + (c.alreadyMember ? ' invite-pop-contact-disabled' : '') + '"'
      + ' data-actor="' + esc(c.actorUrl) + '"' + (c.alreadyMember ? ' disabled' : '') + '>'
      + '<div class="invite-pop-contact-avatar">'
      + avatarContentHtml(c.avatar || '', c.name || initial)
      + '</div>'
      + '<div class="invite-pop-contact-info">'
      + '<div class="invite-pop-contact-name">' + esc(c.name) + '</div>'
      + '<div class="invite-pop-contact-url">' + esc(shortUrl) + '</div>'
      + '</div>'
      + (c.alreadyMember ? '<span class="invite-pop-contact-added">' + esc(lang.members) + '</span>' : '')
      + '</button>';
  });
  listEl.innerHTML = html;

  // Wire contact click handlers
  listEl.querySelectorAll('.invite-pop-contact:not([disabled])').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var actor = btn.getAttribute('data-actor');
      if (actor) doInviteMember(actor);
    });
  });
}

// ==================== Edit Room ====================
function showEditRoomDialog() {
  if (!state.roomDetail) return;
  var overlay = $('edit-room-dialog');
  if (!overlay) return;
  $('edit-room-name').value = state.roomDetail.name || '';
  $('edit-room-desc').value = state.roomDetail.description || '';
  overlay.style.display = 'flex';
}

function hideEditRoomDialog() {
  var overlay = $('edit-room-dialog');
  if (overlay) overlay.style.display = 'none';
}

async function doSaveRoom() {
  if (!state.activeId || !state.roomDetail) return;
  var nameVal = ($('edit-room-name').value || '').trim();
  var descVal = ($('edit-room-desc').value || '').trim();
  if (!nameVal) return;
  var btn = $('edit-room-save');
  btn && (btn.disabled = true, btn.textContent = lang.saving);
  try {
    var updated = await Tapp.federation.updateRoom(state.activeId, { name: nameVal, description: descVal });
    if (updated) state.roomDetail = updated;
    // Sync to room list
    for (var i = 0; i < state.rooms.length; i++) {
      if (state.rooms[i].room_id === state.activeId) {
        state.rooms[i].name = nameVal;
        state.rooms[i].description = descVal;
        break;
      }
    }
    hideEditRoomDialog();
    renderChatHeader();
    renderConvList();
  } catch (e) {
    notifyError(lang.saveFail, e);
  } finally {
    btn && (btn.disabled = false, btn.textContent = lang.save);
  }
}

// ==================== Kick Member ====================
async function doKickMember(actorUrl) {
  if (!state.activeId || state.activeKind !== 'room') return;
  if (!(await aroConfirm(lang.kickConfirm, true))) return;
  try {
    await Tapp.federation.removeMember(state.activeId, actorUrl);
    // Refresh members
    var detail = await Tapp.federation.getRoom(state.activeId);
    if (detail) state.roomDetail = detail;
    var membersRes = await Tapp.federation.getRoomMembers(state.activeId);
    state.members = unwrapRoomMembers(membersRes);
    renderMembers();
    renderChatHeader();
  } catch (e) {
    notifyError(lang.kickFail, e);
  }
}

// ==================== Dissolve Room ====================
async function doDissolveRoom() {
  if (!state.activeId || state.activeKind !== 'room') return;
  if (!(await aroConfirm(lang.dissolveConfirm, true))) return;
  try {
    await unsubscribeRealtime();
    await Tapp.federation.deleteRoom(state.activeId);
    state.activeKind = null;
    state.activeId = null;
    stopPolling();
    $('chat-container').style.display = 'none';
    $('member-panel').style.display = 'none';
    $('member-panel').classList.remove('member-open-mobile');
    $('empty-state').style.display = '';
    $('sidebar').classList.remove('sidebar-hidden-mobile');
    loadConversations();
  } catch (e) {
    notifyError(lang.dissolveFail, e);
  }
}

async function doAcceptChannel() {
  if (!state.activeId || state.activeKind !== 'channel') return;
  try {
    await Tapp.federation.acceptChannel(state.activeId);
    state.channelDetail.status = 'active';
    // Update the channel in the list too
    for (var i = 0; i < state.channels.length; i++) {
      if (state.channels[i].channel_id === state.activeId) {
        state.channels[i].status = 'active'; break;
      }
    }
    renderChatHeader();
    renderConvList();
  } catch (e) {
    notifyError(lang.acceptFail, e);
  }
}

async function doLeaveRoom() {
  if (!state.activeId || state.activeKind !== 'room') return;
  try {
    await unsubscribeRealtime();
    await Tapp.federation.leaveRoom(state.activeId);
    state.activeKind = null;
    state.activeId = null;
    stopPolling();
    $('chat-container').style.display = 'none';
    $('member-panel').style.display = 'none';
    $('member-panel').classList.remove('member-open-mobile');
    $('empty-state').style.display = '';
    $('sidebar').classList.remove('sidebar-hidden-mobile');
    loadConversations();
  } catch (e) { /* ignore */ }
}

// ==================== Create Dialog ====================
function showCreateDialog() {
  var overlay = $('create-dialog');
  if (overlay) overlay.style.display = 'flex';
  switchCreateTab('channel');
}

function hideCreateDialog() {
  var overlay = $('create-dialog');
  if (overlay) overlay.style.display = 'none';
  var channelInput = $('create-channel-input');
  var roomInput = $('create-room-input');
  if (channelInput) channelInput.value = '';
  if (roomInput) roomInput.value = '';
}

function switchCreateTab(tab) {
  var channelTab = $('create-tab-channel');
  var roomTab = $('create-tab-room');
  var channelForm = $('create-form-channel');
  var roomForm = $('create-form-room');
  if (!channelTab) return;
  if (tab === 'channel') {
    channelTab.classList.add('create-tab-active');
    roomTab.classList.remove('create-tab-active');
    channelForm.style.display = '';
    roomForm.style.display = 'none';
  } else {
    roomTab.classList.add('create-tab-active');
    channelTab.classList.remove('create-tab-active');
    roomForm.style.display = '';
    channelForm.style.display = 'none';
  }
}

async function doCreateChannel() {
  var input = $('create-channel-input');
  if (!input) return;
  var remoteActor = input.value.trim();
  if (!remoteActor) return;
  var btn = $('create-channel-btn');
  if (btn) { btn.disabled = true; btn.textContent = lang.creating; }
  try {
    var result = await Tapp.federation.createChannel({ remote_actor: remoteActor });
    hideCreateDialog();
    await loadConversations();
    if (result && result.channel_id) {
      openConversation('channel', result.channel_id);
    }
  } catch (e) {
    console.error('[Aro] createChannel error:', e);
    notifyError(lang.createFail, e);
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = lang.createChannel; }
  }
}

async function doCreateRoom() {
  var input = $('create-room-input');
  if (!input) return;
  var name = input.value.trim();
  if (!name) return;
  var btn = $('create-room-btn');
  if (btn) { btn.disabled = true; btn.textContent = lang.creating; }
  try {
    var result = await Tapp.federation.createRoom({ name: name });
    hideCreateDialog();
    await loadConversations();
    if (result && result.room_id) {
      openConversation('room', result.room_id);
    }
  } catch (e) {
    console.error('[Aro] createRoom error:', e);
    notifyError(lang.createFail, e);
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = lang.createRoom; }
  }
}

// ==================== View Switching ====================
function switchView(view) {
  if (state.isGuest && view !== 'feed') view = 'feed';
  state.currentView = view;
  var views = ['messages', 'feed', 'rings'];
  views.forEach(function (v) {
    var el = $('view-' + v);
    if (el) {
      el.classList.toggle('aro-view-active', v === view);
      if (v !== view) el.style.display = 'none';
      else { el.style.display = ''; el.classList.add('aro-view-active'); }
`

const PAGE_MOD_VIEWS = `\
    }
  });
  // Update nav buttons
  document.querySelectorAll('.aro-nav-item').forEach(function (btn) {
    btn.classList.toggle('aro-nav-active', btn.dataset.view === view);
  });
  // Pause chat poll when not on messages; keep WS for quick resume
  if (view === 'messages') {
    if (state.activeId) startPolling();
  } else {
    stopPolling();
  }
  // Load data for the view
  if (view === 'feed') loadFeed();
  else if (view === 'rings') loadRings();
}

// ==================== Feed View (merged Timeline + Profile) ====================
async function loadFeed() {
  renderFederationIdentity();
  updateFeedProfileHeader();
  return loadFeedSubTab();
}

function updateFeedProfileHeader() {
  if (state.isGuest) {
    state.following = [];
    state.followers = [];
    state.published = [];
    updateFeedHeader();
    return;
  }
  Promise.all([
    Tapp.federation.getFollowing().catch(function () { return { items: [] }; }),
    Tapp.federation.getFollowers().catch(function () { return { items: [] }; }),
    Tapp.federation.getPublished().catch(function () { return { items: [] }; })
  ]).then(function (results) {
    state.following = (results[0] && results[0].items) || [];
    state.followers = (results[1] && results[1].items) || [];
    state.published = (results[2] && results[2].items) || [];
    var el;
    el = $('feed-count-following'); if (el) el.textContent = state.following.length;
    el = $('feed-count-followers'); if (el) el.textContent = state.followers.length;
    el = $('feed-count-published'); if (el) el.textContent = state.published.length;
    el = $('feed-mobile-count-following'); if (el) el.textContent = state.following.length;
    el = $('feed-mobile-count-followers'); if (el) el.textContent = state.followers.length;
    el = $('feed-mobile-count-published'); if (el) el.textContent = state.published.length;
    updateFeedHeader();
  });
}

async function loadFeedSubTab() {
  var sub = state.feedSubTab;
  var followBar = $('feed-follow-bar');
  if (followBar) followBar.style.display = (!state.isGuest && sub === 'following') ? '' : 'none';

  state.feedLoading = true;
  state.feedError = null;
  updateFeedLoadingState();
  updateFeedHeader();
  renderFeedContent();

  try {
    if (sub === 'timeline') {
      var res = typeof Tapp.federation.getFeed === 'function'
        ? await Tapp.federation.getFeed()
        : await Tapp.federation.getTimeline();
      state.timeline = (res && res.items) || [];
    } else if (sub === 'following') {
      var res = await Tapp.federation.getFollowing();
      state.following = (res && res.items) || [];
    } else if (sub === 'followers') {
      var res = await Tapp.federation.getFollowers();
      state.followers = (res && res.items) || [];
    } else if (sub === 'published') {
      var res = await Tapp.federation.getPublished();
      state.published = (res && res.items) || [];
    }
    if (state.feedSubTab !== sub) return;
    state.feedLoaded[sub] = true;
    state.feedError = null;
  } catch (e) {
    if (state.feedSubTab !== sub) return;
    state.feedError = getErrorMessage(e) || lang.feedLoadFail || lang.disconnected || '加载失败';
    console.error('[Aro] loadFeedSubTab error:', e);
  } finally {
    if (state.feedSubTab === sub) {
      state.feedLoading = false;
      updateFeedLoadingState();
      updateFeedHeader();
      renderFeedContent();
    }
  }
}

function updateFeedLoadingState() {
  ['refresh-feed-btn', 'refresh-feed-mobile-btn'].forEach(function (id) {
    var refreshBtn = $(id);
    if (!refreshBtn) return;
    refreshBtn.classList.toggle('feed-refresh-loading', !!state.feedLoading);
    refreshBtn.disabled = !!state.feedLoading;
  });
}

function getFeedTitle(sub) {
  if (state.isGuest) return lang.publicFeed || lang.feedTimeline;
  if (sub === 'following') return lang.feedFollowing;
  if (sub === 'followers') return lang.feedFollowers;
  if (sub === 'published') return lang.feedPublished;
  return lang.feedTimeline;
}

function updateFeedHeader() {
  var title = $('feed-section-title');
  var meta = $('feed-section-meta');
  var sub = state.feedSubTab;
  if (title) title.textContent = getFeedTitle(sub);
  if (!meta) return;
  // 加载/错误状态由骨架屏和空状态呈现，这里只在有数据时给出计数，避免文案闪换
  var items = getFeedItems(sub) || [];
  meta.textContent = (state.feedLoaded[sub] && items.length > 0)
    ? items.length + ' ' + (lang.feedItems || '项')
    : '';
}

function getFeedItems(sub) {
  if (sub === 'following') return state.following;
  if (sub === 'followers') return state.followers;
  if (sub === 'published') return state.published;
  return state.timeline;
}

function getFeedEmptyText(sub) {
  if (sub === 'following') return lang.emptyFollowing;
  if (sub === 'followers') return lang.emptyFollowers;
  if (sub === 'published') return lang.emptyPublished;
  return lang.emptyTimeline;
}

function showFeedEmpty(message, kind) {
  var empty = $('feed-empty');
  if (!empty) return;
  var main = empty.closest('.feed-main');
  if (main) main.classList.add('feed-empty-visible');
  empty.style.display = '';
  empty.classList.toggle('feed-empty-error', kind === 'error');
  empty.classList.toggle('feed-empty-loading', kind === 'loading');
  // 普通空态不重复视图标题（头部已有），只在错误时显示标题行
  var title = $('feed-empty-title');
  if (title) {
    if (kind === 'error') {
      title.style.display = '';
      title.textContent = lang.feedLoadFail || lang.disconnected || '加载失败';
    } else {
      title.style.display = 'none';
      title.textContent = '';
    }
  }
  var text = $('feed-empty-text');
  if (text) text.textContent = message;
}

function renderFeedSkeleton() {
  var html = '';
  for (var i = 0; i < 4; i++) {
    html += '<div class="feed-skeleton-item">'
      + '<div class="feed-skeleton-avatar"></div>'
      + '<div class="feed-skeleton-body">'
      + '<div class="feed-skeleton-line feed-skeleton-line-short"></div>'
      + '<div class="feed-skeleton-line"></div>'
      + '<div class="feed-skeleton-line feed-skeleton-line-mid"></div>'
      + '</div>'
      + '</div>';
  }
  return html;
}

function bindFeedContentActions(content) {
  content.querySelectorAll('[data-action-unfollow]').forEach(function (btn) {
    btn.addEventListener('click', function (e) { e.stopPropagation(); doUnfollow(btn.dataset.actionUnfollow); });
  });
  content.querySelectorAll('[data-action-unpublish]').forEach(function (btn) {
    btn.addEventListener('click', function (e) {
      e.stopPropagation();
      doUnpublish(btn.dataset.contentType, btn.dataset.contentId);
    });
  });
}

function renderFeedContent() {
  var content = $('feed-content');
  var empty = $('feed-empty');
  if (!content) return;
  var main = content.closest('.feed-main');
  if (main) main.classList.remove('feed-empty-visible');

  var sub = state.feedSubTab;
  var items = getFeedItems(sub);
  var hasLoaded = !!state.feedLoaded[sub];
  var html = '';

  if (state.feedLoading && !hasLoaded) {
    content.innerHTML = renderFeedSkeleton();
    if (empty) empty.style.display = 'none';
    return;
  }

  if (state.feedError && !hasLoaded) {
    content.innerHTML = '';
    showFeedEmpty(state.feedError, 'error');
    return;
  }

  if (!items || items.length === 0) {
    content.innerHTML = '';
    showFeedEmpty(getFeedEmptyText(sub), 'empty');
    return;
  }

  if (empty) empty.style.display = 'none';

  if (sub === 'timeline') {
    items.forEach(function (item) {
      html += renderTimelineItem(item);
    });
  } else if (sub === 'following') {
    items.forEach(function (actor) {
      html += renderActorItem(actor, 'following');
    });
  } else if (sub === 'followers') {
    items.forEach(function (actor) {
      html += renderActorItem(actor, 'followers');
    });
  } else if (sub === 'published') {
    items.forEach(function (item) {
      html += renderPublishedItem(item);
    });
  }

  content.innerHTML = html;
  bindFeedContentActions(content);
}

function stripHtmlPreview(html) {
  if (!html) return '';
  return String(html)
    .replace(/<br\\s*\\/?>/gi, '\\n')
    .replace(/<\\/p>/gi, '\\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&')
    .trim();
}

function extractNoteAttachments(contentJson) {
  if (!contentJson) return [];
  var atts = contentJson.attachment || contentJson.attachments || [];
  if (!Array.isArray(atts)) {
    if (atts && typeof atts === 'object') atts = [atts];
    else return [];
  }
  return atts.filter(function (a) { return a && a.url; });
}

function renderTimelineMedia(attachments) {
  if (!attachments || !attachments.length) return '';
  var multi = attachments.length >= 2;
  var h = '<div class="feed-item-media' + (multi ? ' feed-item-media-grid' : ' feed-item-media-single') + '">';
  attachments.forEach(function (att) {
    var url = att.url;
    var mime = (att.mediaType || att.media_type || '').toLowerCase();
    var type = (att.type || '').toLowerCase();
    var isVideo = type === 'video' || mime.indexOf('video/') === 0;
    if (multi) {
      h += '<div class="feed-media-cell">';
      if (isVideo) {
        h += '<video src="' + esc(url) + '" controls playsinline preload="metadata"></video>';
      } else {
        h += '<img src="' + esc(url) + '" alt="" loading="lazy" />';
      }
      h += '</div>';
    } else if (isVideo) {
      h += '<video src="' + esc(url) + '" controls playsinline preload="metadata"></video>';
    } else {
      h += '<img src="' + esc(url) + '" alt="" loading="lazy" />';
    }
  });
  h += '</div>';
  return h;
}

function renderTimelineItem(item) {
  var actor = item.actor || {};
  var name = actor.display_name || actor.username || '?';
  var handle = actor.username ? '@' + actor.username + (actor.domain ? '@' + actor.domain : '') : '';
  var ts = '';
  try { ts = timeAgo(item.created_at || item.received_at || item.timestamp); } catch (e) {}
  var contentJson = item.content_json || item.content || null;
  var text = '';
  if (contentJson) {
    text = stripHtmlPreview(
      (contentJson.source && contentJson.source.content) ||
      contentJson.content ||
      contentJson.summary ||
      contentJson.name ||
      ''
    );
  }
  if (!text && item.content_preview) text = stripHtmlPreview(item.content_preview);
  var attachments = extractNoteAttachments(contentJson);
  var h = '<div class="feed-item">';
  h += '<div class="feed-item-avatar">' + avatarContentHtml(actor.avatar_url || '', name) + '</div>';
  h += '<div class="feed-item-body">';
  h += '<div class="feed-item-header">';
  h += '<span class="feed-item-name">' + esc(name) + '</span>';
  if (handle) h += '<span class="feed-item-handle">' + esc(handle) + '</span>';
  if (ts) h += '<span class="feed-item-sep">&middot;</span><span class="feed-item-time">' + esc(ts) + '</span>';
  h += '</div>';
  if (text) {
    h += '<div class="feed-item-text">' + esc(text) + '</div>';
  }
  h += renderTimelineMedia(attachments);
  h += '</div></div>';
  return h;
}

function pendingStatusLabel(status) {
  if (!status || status === 'accepted') return '';
  if (status === 'pending') return lang.pendingConfirm || lang.pending || 'pending';
  return status;
}

function renderActorItem(actor, context) {
  var name = actor.display_name || actor.username || '?';
  var handle = actor.username ? '@' + actor.username + (actor.domain ? '@' + actor.domain : '') : actor.domain || '';
  var h = '<div class="feed-item">';
  h += '<div class="feed-item-avatar">' + avatarContentHtml(actor.avatar_url || '', name) + '</div>';
  h += '<div class="feed-item-body">';
  h += '<div class="feed-item-header">';
  h += '<span class="feed-item-name">' + esc(name) + '</span>';
  if (handle) h += '<span class="feed-item-handle">' + esc(handle) + '</span>';
  h += '</div>';
  if (actor.bio) h += '<div class="feed-item-text">' + esc(actor.bio) + '</div>';
  // Status + action — localize non-accepted (pending) badge
  h += '<div class="feed-item-actions">';
  if (actor.status && actor.status !== 'accepted') {
    h += '<span class="aro-badge aro-badge-pending">' + esc(pendingStatusLabel(actor.status)) + '</span>';
  }
  if (context === 'following') {
    h += '<button class="feed-item-action feed-item-action-danger" data-action-unfollow="' + esc(actor.actor_url || '') + '">'
      + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>'
      + esc(lang.unfollowBtn) + '</button>';
  }
  h += '</div></div></div>';
  return h;
}

/** 已发布内容的可读类型名 */
function publishedTypeLabel(type) {
  var map = {
    'note': lang.composePost,
    'tapp': lang.attachTapp,
    'brew-article': lang.attachBrew,
    'library': lang.attachLibrary,
    'report': lang.attachReport,
  };
  return map[type] || type || '';
}

function renderPublishedItem(item) {
  var typeIcons = { 'report': SVG_ICONS.report, 'brew-article': SVG_ICONS.memo, 'tapp': SVG_ICONS.tapp, 'library': SVG_ICONS.library, 'note': SVG_ICONS.page };
  var icon = typeIcons[item.content_type] || SVG_ICONS.page;
  var dateStr = '';
  try { dateStr = timeAgo(item.published_at); } catch (e) {}
  // 优先展示内容本身（标题/摘要/预览），而不是裸 ID
  var preview = stripHtmlPreview(item.title || item.name || item.summary || item.content_preview || '');
  var h = '<div class="feed-item">';
  h += '<div class="feed-item-icon">' + icon + '</div>';
  h += '<div class="feed-item-body">';
  h += '<div class="feed-item-header">';
  h += '<span class="feed-item-name">' + esc(publishedTypeLabel(item.content_type)) + '</span>';
  if (dateStr) h += '<span class="feed-item-sep">&middot;</span><span class="feed-item-time">' + esc(dateStr) + '</span>';
  h += '</div>';
  if (preview) h += '<div class="feed-item-text">' + esc(preview) + '</div>';
  h += '<div class="feed-item-actions">';
  h += '<button class="feed-item-action feed-item-action-danger" data-action-unpublish data-content-type="' + esc(item.content_type) + '" data-content-id="' + esc(item.content_id) + '">'
    + '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>'
    + esc(lang.removeBtn) + '</button>';
  h += '</div></div></div>';
  return h;
}

function timeAgo(iso) {
  if (!iso) return '';
  var d = new Date(iso);
  var now = new Date();
  var sec = Math.floor((now - d) / 1000);
  if (sec < 60) return sec + 's';
  var min = Math.floor(sec / 60);
  if (min < 60) return min + 'm';
  var hr = Math.floor(min / 60);
  if (hr < 24) return hr + 'h';
  var day = Math.floor(hr / 24);
  if (day < 30) return day + 'd';
  try { return d.toLocaleDateString(currentLocale, { month: 'short', day: 'numeric' }); } catch (e) { return day + 'd'; }
}

function switchFeedSubTab(sub) {
  state.feedSubTab = sub;
  updateFeedHeader();
  // Update sidebar nav
  document.querySelectorAll('.feed-nav-item').forEach(function (btn) {
    btn.classList.toggle('feed-nav-active', btn.dataset.sub === sub);
  });
  // Update mobile tabs
  document.querySelectorAll('.feed-mobile-tab').forEach(function (btn) {
    btn.classList.toggle('feed-mobile-tab-active', btn.dataset.sub === sub);
  });
  loadFeedSubTab();
}

async function doFollow() {
  var input = $('feed-follow-input');
  var btn = $('feed-follow-btn');
  if (!input) return;
  var target = input.value.trim();
  if (!target) return;
  if (btn) { btn.disabled = true; }
  try {
    await Tapp.federation.follow(target);
    input.value = '';
    // Refresh following list; auto-accept is remote (no manual approve UI).
    if (state.feedSubTab !== 'following') {
      state.feedSubTab = 'following';
      switchFeedSubTab('following');
    } else {
      loadFeedSubTab();
    }
    updateFeedProfileHeader();
    try {
      Tapp.ui.showNotification({
        title: lang.followBtn || 'Follow',
        message: lang.followQueued || '',
        type: 'info'
      });
    } catch (e2) { /* ignore */ }
  } catch (e) {
    notifyError(lang.followFail, e);
  } finally {
    if (btn) btn.disabled = false;
  }
}

// ==================== Feed composer (freeform Note) ====================
var composeAttachments = []; // { file, previewUrl, kind: 'image'|'video' }

function updateComposeButtonVisibility() {
  var show = state.isGuest ? 'none' : '';
  var btn = $('feed-compose-btn');
  if (btn) btn.style.display = show;
  var mobile = $('feed-compose-mobile-btn');
  if (mobile) mobile.style.display = show;
}

function openComposer() {
  if (state.isGuest) return;
  var el = $('feed-composer');
  if (!el) return;
  el.style.display = '';
  var ta = $('feed-compose-text');
  if (ta) { ta.focus(); }
}

function closeComposer() {
  var el = $('feed-composer');
  if (el) el.style.display = 'none';
  var ta = $('feed-compose-text');
  if (ta) ta.value = '';
  composeAttachments.forEach(function (a) {
    if (a.previewUrl) try { URL.revokeObjectURL(a.previewUrl); } catch (e) {}
  });
  composeAttachments = [];
  renderComposePreviews();
}

function renderComposePreviews() {
  var box = $('feed-compose-previews');
  if (!box) return;
  if (!composeAttachments.length) {
    box.innerHTML = '';
    return;
  }
  var h = '';
  composeAttachments.forEach(function (a, idx) {
    h += '<div class="feed-compose-preview">';
    if (a.kind === 'video') {
      h += '<video src="' + esc(a.previewUrl) + '" muted></video>';
    } else {
      h += '<img src="' + esc(a.previewUrl) + '" alt="" />';
    }
    h += '<button type="button" class="feed-compose-preview-remove" data-compose-remove="' + idx + '" aria-label="remove">&times;</button>';
    h += '</div>';
  });
  box.innerHTML = h;
  box.querySelectorAll('[data-compose-remove]').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var i = parseInt(btn.getAttribute('data-compose-remove'), 10);
      if (isNaN(i) || i < 0 || i >= composeAttachments.length) return;
      var removed = composeAttachments.splice(i, 1)[0];
      if (removed && removed.previewUrl) try { URL.revokeObjectURL(removed.previewUrl); } catch (e) {}
      renderComposePreviews();
    });
  });
}

function addComposeFiles(fileList, forceKind) {
  if (!fileList || !fileList.length) return;
  var maxImage = 10 * 1024 * 1024;
  var maxVideo = 50 * 1024 * 1024;
  for (var i = 0; i < fileList.length; i++) {
    if (composeAttachments.length >= 8) break;
    var file = fileList[i];
    var mime = (file.type || '').toLowerCase();
    var kind = forceKind || (mime.indexOf('video/') === 0 ? 'video' : 'image');
    if (kind === 'image' && mime && mime.indexOf('image/') !== 0) {
      notifyError(lang.mediaUnsupported || 'Unsupported');
      continue;
    }
    if (kind === 'video' && mime && mime.indexOf('video/') !== 0) {
      notifyError(lang.mediaUnsupported || 'Unsupported');
      continue;
    }
    var max = kind === 'video' ? maxVideo : maxImage;
    if (file.size > max) {
      notifyError(lang.mediaTooLarge || lang.fileTooLarge || 'Too large');
      continue;
    }
    composeAttachments.push({
      file: file,
      previewUrl: URL.createObjectURL(file),
      kind: kind
    });
  }
  renderComposePreviews();
}

function fileToDataUrl(file) {
  return new Promise(function (resolve, reject) {
    var reader = new FileReader();
    reader.onload = function () { resolve(reader.result); };
    reader.onerror = function () { reject(new Error('read failed')); };
    reader.readAsDataURL(file);
  });
}

async function uploadComposeMedia(entry) {
  var file = entry.file;
  if (typeof Tapp.federation.uploadMedia === 'function') {
    var dataUrl = await fileToDataUrl(file);
    var res = await Tapp.federation.uploadMedia({
      data: dataUrl,
      name: file.name || 'upload.bin',
      mime: file.type || (entry.kind === 'video' ? 'video/mp4' : 'image/jpeg')
    });
    return res;
  }
  // Fallback: publish path unavailable
  throw new Error('uploadMedia not available');
}

async function publishComposeNote() {
  if (state.isGuest) return;
  var ta = $('feed-compose-text');
  var text = ta ? ta.value.trim() : '';
  if (!text && !composeAttachments.length) {
    notifyError(lang.composeEmpty || 'Empty');
    return;
  }
  var publishBtn = $('feed-compose-publish');
  var cancelBtn = $('feed-compose-cancel');
  var setBusy = function (busy) {
    if (publishBtn) {
      publishBtn.disabled = busy;
      publishBtn.textContent = busy
        ? (lang.composePublishing || '…')
        : (lang.composePublish || 'Publish');
    }
    if (cancelBtn) cancelBtn.disabled = busy;
  };
  setBusy(true);
  try {
    var attachments = [];
    for (var i = 0; i < composeAttachments.length; i++) {
      if (publishBtn) publishBtn.textContent = lang.composeUploading || '…';
      var uploaded = await uploadComposeMedia(composeAttachments[i]);
      attachments.push({
        url: uploaded.url,
        media_type: uploaded.media_type || uploaded.mediaType || composeAttachments[i].file.type,
        name: uploaded.name || composeAttachments[i].file.name
      });
    }
    if (typeof Tapp.federation.createNote === 'function') {
      await Tapp.federation.createNote({
        text: text,
        attachments: attachments,
        visibility: 'public'
      });
    } else {
      await Tapp.federation.publish({
        content_type: 'note',
        text: text,
        attachments: attachments,
        visibility: 'public'
      });
    }
    closeComposer();
    try {
      Tapp.ui.showNotification({ title: lang.composeSuccess || 'OK', type: 'success' });
    } catch (e2) {}
    state.feedLoaded.timeline = false;
    state.feedLoaded.published = false;
    if (state.feedSubTab !== 'timeline') {
      switchFeedSubTab('timeline');
    } else {
      loadFeedSubTab();
    }
    updateFeedProfileHeader();
  } catch (e) {
    notifyError(lang.composeFail || lang.unpublishFail || 'Fail', e);
  } finally {
    setBusy(false);
  }
}

async function doUnfollow(actorUrl) {
  try {
    await Tapp.federation.unfollow(actorUrl);
    loadFeedSubTab();
    updateFeedProfileHeader();
  } catch (e) {
    notifyError(lang.unfollowFail, e);
  }
}

async function doUnpublish(contentType, contentId) {
  try {
    await Tapp.federation.unpublish({ content_type: contentType, content_id: contentId });
    loadFeedSubTab();
    updateFeedProfileHeader();
  } catch (e) {
    notifyError(lang.unpublishFail, e);
  }
}

// ==================== Rings View ====================
async function loadRings() {
  try {
    var res = await Tapp.federation.getRings();
    state.rings = (res && res.rings) || [];
    state.activeRingId = null;
    renderRingsSidebar();
    hideRingDetail();
  } catch (e) { console.error('[Aro] loadRings error:', e); }
}

function renderRingsSidebar() {
  var list = $('ring-list');
  if (!list) return;
  if (state.rings.length === 0) {
    list.innerHTML = '<div class="conv-empty conv-empty-fill"><span id="ring-empty-text">' + esc(lang.emptyRings) + '</span></div>';
    return;
  }
  var typeIcons = { 'brew-recommend': SVG_ICONS.coffee, 'tapp-store': SVG_ICONS.puzzle, 'library-exchange': SVG_ICONS.library, 'instance-directory': SVG_ICONS.globe };
  var html = '';
  state.rings.forEach(function (ring) {
    var icon = typeIcons[ring.ring_type] || SVG_ICONS.ring;
    var name = ring.ring_name || ring.ring_id;
    var peerText = (ring.peer_count || 0) + ' ' + lang.peers;
    var activeClass = state.activeRingId === ring.ring_id ? ' conv-active' : '';
    html += '<button class="conv-item' + activeClass + '" data-ring-id="' + esc(ring.ring_id) + '">'
      + '<div class="conv-avatar avatar-room" style="border-radius:8px;font-size:16px">' + icon + '</div>'
      + '<div class="conv-info">'
      + '<div class="conv-name">' + esc(name) + '</div>'
      + '<div class="conv-subtitle">' + esc(ringTypeLabel(ring.ring_type)) + ' · ' + esc(peerText) + '</div>'
      + '</div>'
      + '</button>';
  });
  list.innerHTML = html;
  list.querySelectorAll('.conv-item').forEach(function (btn) {
    btn.addEventListener('click', function () {
      openRingDetail(btn.dataset.ringId);
    });
  });
}

async function doCreateRing() {
  if (!requireAdminAction()) return;
  var input = $('ring-name-input');
  var btn = $('create-ring-btn');
  if (!input) return;
  var name = input.value.trim();
  if (!name) return;
  var type = ($('ring-type-select') || {}).value || 'brew-recommend';
  if (btn) { btn.disabled = true; btn.textContent = lang.creating; }
  try {
    await Tapp.federation.createRing({ name: name, ring_type: type });
    input.value = '';
    var d = $('ring-create-dialog'); if (d) d.style.display = 'none';
    loadRings();
  } catch (e) {
    notifyError(lang.createRingFail, e);
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = lang.createRingBtn; }
  }
}

async function doLeaveRing(ringId) {
  if (!requireAdminAction()) return;
  try {
    await Tapp.federation.leaveRing(ringId);
    hideRingDetail();
    loadRings();
  } catch (e) {
    notifyError(lang.leaveRingFail, e);
  }
}

// ==================== Ring Detail (inline panel) ====================
function openRingDetail(ringId) {
  state.activeRingId = ringId;
  state.ringDetail = null;
  state.ringPeers = [];
  // Update sidebar active
  var list = $('ring-list');
  if (list) list.querySelectorAll('.conv-item').forEach(function (btn) {
    btn.classList.toggle('conv-active', btn.dataset.ringId === ringId);
  });
  // Show detail panel
  $('ring-empty-state').style.display = 'none';
  $('ring-detail').style.display = '';
  // Mobile
  $('ring-sidebar').classList.add('sidebar-hidden-mobile');
  var main = $('ring-detail').closest('.panel-main');
  if (main) main.classList.add('panel-main-show-mobile');
  loadRingDetail(ringId);
}

function hideRingDetail() {
  state.activeRingId = null;
  state.ringDetail = null;
  state.ringPeers = [];
  var detail = $('ring-detail');
  if (detail) detail.style.display = 'none';
  var empty = $('ring-empty-state');
  if (empty) empty.style.display = '';
  var sidebar = $('ring-sidebar');
  if (sidebar) sidebar.classList.remove('sidebar-hidden-mobile');
  var main = detail ? detail.closest('.panel-main') : null;
  if (main) main.classList.remove('panel-main-show-mobile');
}

async function loadRingDetail(ringId) {
  try {
    var results = await Promise.all([
      Tapp.federation.getRing(ringId),
      Tapp.federation.getRingPeers(ringId)
    ]);
    if (state.activeRingId !== ringId) return; // user closed
    state.ringDetail = results[0];
    state.ringPeers = (results[1] && results[1].peers) || [];
    renderRingDetail();
  } catch (e) {
    console.error('[Aro] loadRingDetail error:', e);
  }
}

function renderRingDetail() {
  var ring = state.ringDetail;
  if (!ring) return;
  var typeIcons = { 'brew-recommend': SVG_ICONS.coffee, 'tapp-store': SVG_ICONS.puzzle, 'library-exchange': SVG_ICONS.library, 'instance-directory': SVG_ICONS.globe };
  var iconEl = $('ring-detail-icon');
  if (iconEl) iconEl.innerHTML = typeIcons[ring.ring_type] || SVG_ICONS.ring;
  var nameEl = $('ring-detail-name');
  if (nameEl) nameEl.textContent = ring.ring_name || ring.ring_id;
  var metaEl = $('ring-detail-meta');
  if (metaEl) {
    var parts = [];
    parts.push('<span class="meta-badge">' + esc(ringTypeLabel(ring.ring_type)) + '</span>');
    parts.push('<span class="meta-badge">' + esc(state.ringPeers.length + ' ' + lang.peers) + '</span>');
    if (ring.last_sync_at) {
      try { parts.push('<span class="meta-badge">' + esc(timeAgo(ring.last_sync_at)) + '</span>'); } catch (e) {}
    }
    metaEl.innerHTML = parts.join('');
  }

  // Sync / leave labels
  var syncLabel = $('ring-sync-label');
  if (syncLabel) syncLabel.textContent = lang.syncBtn;
  var leaveLabel = $('ring-leave-label');
  if (leaveLabel) leaveLabel.textContent = lang.leaveBtn;

  // Peer input
  var peerInput = $('ring-peer-input');
  if (peerInput) peerInput.placeholder = lang.addPeerPlaceholder;
  var addPeerBtn = $('ring-add-peer-btn');
  if (addPeerBtn) addPeerBtn.textContent = lang.addPeerBtn;
  applyAdminControls();

  // Render peers as member-item style
  var peersList = $('ring-peers-list');
  var peersEmpty = $('ring-peers-empty');
  if (!peersList) return;

  if (state.ringPeers.length === 0) {
    peersList.innerHTML = '';
    if (peersEmpty) { peersEmpty.style.display = ''; peersEmpty.querySelector('span').textContent = lang.emptyPeers; }
    return;
  }
  if (peersEmpty) peersEmpty.style.display = 'none';

  var html = '';
  state.ringPeers.forEach(function (peer) {
    var url = peer.actor_url || peer.peer_url || peer.url || peer;
    var urlStr = typeof url === 'string' ? url : JSON.stringify(url);
    var initial = SVG_ICONS.globe;
    html += '<div class="member-item">'
      + '<div class="member-avatar" style="border-radius:6px;font-size:12px">' + initial + '</div>'
      + '<div class="member-info">'
      + '<div class="member-name">' + esc(urlStr) + '</div>'
      + '</div>'
      + (state.isAdmin ? '<button class="member-kick ring-peer-remove-btn" data-peer-url="' + esc(typeof url === 'string' ? url : '') + '" title="Remove">'
      + '<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>'
      + '</button>' : '')
      + '</div>';
  });
  peersList.innerHTML = html;
  applyAdminControls();

  peersList.querySelectorAll('.ring-peer-remove-btn').forEach(function (btn) {
    btn.addEventListener('click', function () {
      doRemovePeer(btn.dataset.peerUrl);
    });
  });
}

async function doAddPeer() {
  if (!requireAdminAction()) return;
  var input = $('ring-peer-input');
  var btn = $('ring-add-peer-btn');
  if (!input || !state.activeRingId) return;
  var peerUrl = input.value.trim();
  if (!peerUrl) return;
  if (btn) btn.disabled = true;
  try {
    await Tapp.federation.addPeer(state.activeRingId, { peer: peerUrl });
    input.value = '';
    loadRingDetail(state.activeRingId);
  } catch (e) {
    notifyError(lang.addPeerFail, e);
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function doRemovePeer(peerUrl) {
  if (!requireAdminAction()) return;
  if (!state.activeRingId || !peerUrl) return;
  try {
    await Tapp.federation.removePeer(state.activeRingId, peerUrl);
    loadRingDetail(state.activeRingId);
  } catch (e) {
    notifyError(lang.removePeerFail, e);
  }
}

async function doTriggerSync() {
  if (!requireAdminAction()) return;
  if (!state.activeRingId) return;
  var btn = $('ring-sync-btn');
  var statusEl = $('ring-sync-status');
  if (btn) btn.disabled = true;
  if (statusEl) { statusEl.style.display = ''; statusEl.className = 'ring-sync-bar'; statusEl.textContent = lang.syncing; }
  try {
    await Tapp.federation.triggerSync(state.activeRingId);
    if (statusEl) { statusEl.className = 'ring-sync-bar ring-sync-ok'; statusEl.textContent = lang.syncSuccess; }
    // Refresh detail after sync
    loadRingDetail(state.activeRingId);
  } catch (e) {
    if (statusEl) { statusEl.className = 'ring-sync-bar ring-sync-err'; statusEl.textContent = lang.syncFail + errorSuffix(e); }
  } finally {
    if (btn) btn.disabled = false;
    // Auto-hide status after 3s
    setTimeout(function () {
      if (statusEl) statusEl.style.display = 'none';
    }, 3000);
  }
}

// ==================== Event Binding ====================
function bindEvents() {
  // Aro nav
  document.querySelectorAll('.aro-nav-item').forEach(function (btn) {
    btn.addEventListener('click', function () { switchView(btn.dataset.view); });
  });

  // Ring create dialog
  var ringCreateOpenBtn = $('ring-create-open-btn');
  if (ringCreateOpenBtn) ringCreateOpenBtn.addEventListener('click', function () {
    if (!requireAdminAction()) return;
`

const PAGE_MOD_EVENTS = `\
    var d = $('ring-create-dialog'); if (d) d.style.display = '';
  });
  var ringCreateClose = $('ring-create-close');
  if (ringCreateClose) ringCreateClose.addEventListener('click', function () {
    var d = $('ring-create-dialog'); if (d) d.style.display = 'none';
  });
  var ringCreateOverlay = $('ring-create-dialog');
  if (ringCreateOverlay) ringCreateOverlay.addEventListener('click', function (e) {
    if (e.target === ringCreateOverlay) ringCreateOverlay.style.display = 'none';
  });

  // Ring create submit
  var createRingBtn = $('create-ring-btn');
  if (createRingBtn) createRingBtn.addEventListener('click', doCreateRing);
  var ringNameInput = $('ring-name-input');
  if (ringNameInput) ringNameInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); doCreateRing(); }
  });

  // Ring detail inline panel events
  var ringBackBtn = $('ring-back-btn');
  if (ringBackBtn) ringBackBtn.addEventListener('click', hideRingDetail);
  var ringSyncBtn = $('ring-sync-btn');
  if (ringSyncBtn) ringSyncBtn.addEventListener('click', doTriggerSync);
  var ringManageBtn = $('ring-manage-btn');
  if (ringManageBtn) ringManageBtn.addEventListener('click', function () {
    var dd = $('ring-manage-dropdown');
    if (dd) dd.classList.toggle('open');
  });
  var ringLeaveBtn2 = $('ring-leave-btn');
  if (ringLeaveBtn2) ringLeaveBtn2.addEventListener('click', async function () {
    var dd = $('ring-manage-dropdown'); if (dd) dd.classList.remove('open');
    if (state.activeRingId && (await aroConfirm(lang.leaveRingConfirm, true))) {
      doLeaveRing(state.activeRingId);
    }
  });
  var ringAddPeerBtn = $('ring-add-peer-btn');
  if (ringAddPeerBtn) ringAddPeerBtn.addEventListener('click', doAddPeer);
  var ringPeerInput = $('ring-peer-input');
  if (ringPeerInput) ringPeerInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); doAddPeer(); }
  });

  // Feed: refresh, tabs, follow, stat clicks
  var refreshFeedBtn = $('refresh-feed-btn');
  if (refreshFeedBtn) refreshFeedBtn.addEventListener('click', function () { loadFeed(); });
  var refreshFeedMobileBtn = $('refresh-feed-mobile-btn');
  if (refreshFeedMobileBtn) refreshFeedMobileBtn.addEventListener('click', function () { loadFeed(); });
  document.querySelectorAll('.feed-nav-item').forEach(function (btn) {
    btn.addEventListener('click', function () { switchFeedSubTab(btn.dataset.sub); });
  });
  document.querySelectorAll('.feed-mobile-tab').forEach(function (btn) {
    btn.addEventListener('click', function () { switchFeedSubTab(btn.dataset.sub); });
  });
  var feedFollowBtn = $('feed-follow-btn');
  if (feedFollowBtn) feedFollowBtn.addEventListener('click', doFollow);
  var feedFollowInput = $('feed-follow-input');
  if (feedFollowInput) feedFollowInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); doFollow(); }
  });
  // Feed freeform note composer
  function toggleComposerPanel() {
    var composer = $('feed-composer');
    if (composer && composer.style.display !== 'none') closeComposer();
    else openComposer();
  }
  var composeOpenBtn = $('feed-compose-btn');
  if (composeOpenBtn) composeOpenBtn.addEventListener('click', toggleComposerPanel);
  var composeMobileBtn = $('feed-compose-mobile-btn');
  if (composeMobileBtn) composeMobileBtn.addEventListener('click', toggleComposerPanel);
  var composeCancel = $('feed-compose-cancel');
  if (composeCancel) composeCancel.addEventListener('click', closeComposer);
  var composePublish = $('feed-compose-publish');
  if (composePublish) composePublish.addEventListener('click', publishComposeNote);
  var composeImageBtn = $('feed-compose-image-btn');
  var composeImageInput = $('feed-compose-image-input');
  if (composeImageBtn && composeImageInput) {
    composeImageBtn.addEventListener('click', function () { composeImageInput.click(); });
    composeImageInput.addEventListener('change', function () {
      addComposeFiles(composeImageInput.files, 'image');
      composeImageInput.value = '';
    });
  }
  var composeVideoBtn = $('feed-compose-video-btn');
  var composeVideoInput = $('feed-compose-video-input');
  if (composeVideoBtn && composeVideoInput) {
    composeVideoBtn.addEventListener('click', function () { composeVideoInput.click(); });
    composeVideoInput.addEventListener('change', function () {
      addComposeFiles(composeVideoInput.files, 'video');
      composeVideoInput.value = '';
    });
  }
  document.querySelectorAll('[data-fed-toggle]').forEach(function (summary) {
    summary.addEventListener('click', function (e) {
      if (e.target && (e.target.closest('[data-copy-fed]') || e.target.closest('[data-fed-toggle-button]'))) return;
      toggleFeedProfileSummary(summary.closest('[data-fed-profile]'));
    });
    summary.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        toggleFeedProfileSummary(summary.closest('[data-fed-profile]'));
      }
    });
  });
  document.querySelectorAll('[data-fed-toggle-button]').forEach(function (btn) {
    btn.addEventListener('click', function (e) {
      e.preventDefault();
      e.stopPropagation();
      toggleFeedProfileDetails(btn.closest('[data-fed-profile]'));
    });
  });
  document.querySelectorAll('[data-copy-fed]').forEach(function (btn) {
    btn.addEventListener('click', function (e) {
      e.preventDefault();
      e.stopPropagation();
      copyFederationIdentity(btn.dataset.copyFed);
    });
  });
  document.addEventListener('click', function (e) {
    if (e.target && e.target.closest('[data-fed-profile]')) return;
    closeFeedProfilePopovers();
  });
  window.addEventListener('resize', function () { closeFeedProfilePopovers(); });

  // Messenger events
  var sendBtn = $('send-btn');
  if (sendBtn) sendBtn.addEventListener('click', doSend);

  var attachBtn = $('attach-btn');
  if (attachBtn) attachBtn.addEventListener('click', function (e) { e.stopPropagation(); toggleAttachMenu(); });

  var attachImageInput = $('attach-image-input');
  if (attachImageInput) attachImageInput.addEventListener('change', function () { if (this.files[0]) handleFileSelect(this.files[0], 'image'); });

  var attachFileInput = $('attach-file-input');
  if (attachFileInput) attachFileInput.addEventListener('change', function () { if (this.files[0]) handleFileSelect(this.files[0]); });

  var input = $('msg-input');
  if (input) {
    input.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); doSend(); }
    });
    input.addEventListener('input', function () {
      autoResizeInput(this);
      updateSendState();
    });
  }
  updateSendState();

  var backBtn = $('back-btn');
  if (backBtn) {
    backBtn.addEventListener('click', function () {
      $('sidebar').classList.remove('sidebar-hidden-mobile');
      $('chat-container').style.display = 'none';
      $('member-panel').style.display = 'none';
      $('member-panel').classList.remove('member-open-mobile');
      $('empty-state').style.display = '';
      clearPendingAttach();
      closeAttachMenu();
      stopPolling();
      unsubscribeRealtime();
      state.activeKind = null;
      state.activeId = null;
      state.messages = [];
      state.messagesFp = '';
    });
  }

  var memberBackBtn = $('member-back-btn');
  if (memberBackBtn) {
    memberBackBtn.addEventListener('click', function () {
      closeMemberPanel();
    });
  }

  // Create dialog events
  var createBtn = $('create-btn');
  if (createBtn) createBtn.addEventListener('click', showCreateDialog);

  var overlay = $('create-dialog');
  if (overlay) overlay.addEventListener('click', function (e) {
    if (e.target === overlay) hideCreateDialog();
  });

  var closeDialogBtn = $('create-dialog-close');
  if (closeDialogBtn) closeDialogBtn.addEventListener('click', hideCreateDialog);

  var tabChannel = $('create-tab-channel');
  if (tabChannel) tabChannel.addEventListener('click', function () { switchCreateTab('channel'); });

  var tabRoom = $('create-tab-room');
  if (tabRoom) tabRoom.addEventListener('click', function () { switchCreateTab('room'); });

  var createChannelBtn = $('create-channel-btn');
  if (createChannelBtn) createChannelBtn.addEventListener('click', doCreateChannel);

  var createRoomBtn = $('create-room-btn');
  if (createRoomBtn) createRoomBtn.addEventListener('click', doCreateRoom);

  // Enter key in create inputs
  var channelInput = $('create-channel-input');
  if (channelInput) channelInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); doCreateChannel(); }
  });
  var roomInput = $('create-room-input');
  if (roomInput) roomInput.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); doCreateRoom(); }
  });

  // Invite popover events
  var inviteToggle = $('invite-toggle');
  if (inviteToggle) inviteToggle.addEventListener('click', toggleInvitePopover);

  // Edit room dialog events
  var editRoomOverlay = $('edit-room-dialog');
  if (editRoomOverlay) editRoomOverlay.addEventListener('click', function (e) {
    if (e.target === editRoomOverlay) hideEditRoomDialog();
  });
  var editRoomCloseBtn = $('edit-room-close');
  if (editRoomCloseBtn) editRoomCloseBtn.addEventListener('click', hideEditRoomDialog);
  var editRoomSaveBtn = $('edit-room-save');
  if (editRoomSaveBtn) editRoomSaveBtn.addEventListener('click', doSaveRoom);
}

// ==================== Init ====================
async function init() {
  try {
    var user = await Tapp.context.getUser();
    var actorUrl = user ? normalizeFederationUrl(user.actor_url) : '';
    if (actorUrl) state.localActorUrl = actorUrl;
  } catch (e) { /* ignore */ }

  try {
`

const PAGE_MOD_INDEX = `\
    var localeRes = await Tapp.ui.getLocale();
    if (localeRes) setLocale(localeRes);
  } catch (e) { /* ignore */ }

  try {
    var settings = await Tapp.settings.getAll();
    if (settings && settings.pollInterval) {
      state.pollInterval = Math.max(5, Math.min(120, settings.pollInterval)) * 1000;
    }
    if (settings && typeof settings.notifyOnMessage !== 'undefined') {
      state.notifyOnMessage = !!settings.notifyOnMessage;
    }
  } catch (e) { /* ignore */ }

  // Load tapp acceptance states from storage
  try {
    var allStorage = await Tapp.storage.getAll();
    if (allStorage) {
      Object.keys(allStorage).forEach(function (k) {
        if (k.indexOf('tapp_accept_') === 0) {
          state.tappAcceptMap[k] = allStorage[k];
        }
      });
    }
  } catch (e) { /* ignore */ }

  await loadUserRole();
  await loadFederationIdentity();
  applyLabels();

  // -- Populate feed profile header from user context + federation identity --
  try {
    var user = await Tapp.context.getUser();
    if (user) {
      synthesizeFederationIdentityFromUser(user);
      // Merge federation identity avatar/name when context is empty
      if (state.identity) {
        if (!user.avatar_url && !user.avatar && state.identity.avatar_url) {
          user.avatar_url = state.identity.avatar_url;
          user.avatar = state.identity.avatar_url;
        }
        if (!user.display_name && state.identity.display_name) {
          user.display_name = state.identity.display_name;
        }
      }
      renderFeedProfileUser(user);
      // Update nav feed tab avatar + username
      var navAvatar = $('nav-feed-avatar');
      if (navAvatar) {
        var navAvatarUrl = user.avatar_url || user.avatar || '';
        navAvatar.innerHTML = avatarContentHtml(navAvatarUrl, user.display_name || user.username || '?');
      }
      var navName = $('nav-feed-label');
      if (navName) navName.textContent = user.display_name || user.username || '';
    } else if (state.identity) {
      renderFeedProfileUser({
        username: state.identity.username,
        display_name: state.identity.display_name || state.identity.username,
        avatar_url: state.identity.avatar_url || '',
        avatar: state.identity.avatar_url || '',
      });
      var navAvatarFallback = $('nav-feed-avatar');
      if (navAvatarFallback) {
        navAvatarFallback.innerHTML = avatarContentHtml(
          state.identity.avatar_url || '',
          state.identity.display_name || state.identity.username || '?'
        );
      }
      var navNameFallback = $('nav-feed-label');
      if (navNameFallback) {
        navNameFallback.textContent = state.identity.display_name || state.identity.username || '';
      }
    }
  } catch (e) {
    if (state.identity) {
      try {
        renderFeedProfileUser({
          username: state.identity.username,
          display_name: state.identity.display_name || state.identity.username,
          avatar_url: state.identity.avatar_url || '',
          avatar: state.identity.avatar_url || '',
        });
      } catch (e2) { /* ignore */ }
    }
  }
  renderFederationIdentity();
  applyAdminControls();
  applyRoleControls();

  bindEvents();
  if (!state.isGuest) {
    bindRealtimeListeners();
    await loadConversations();
  }
  await loadFeed();

  // Handle launch params
  var launchParams = window._TAPP_LAUNCH_PARAMS || {};
  if (!state.isGuest && launchParams.view && ['messages', 'feed', 'rings'].indexOf(launchParams.view) !== -1) {
    switchView(launchParams.view);
  } else if (launchParams.view === 'timeline' || launchParams.view === 'profile') {
    switchView('feed');
  }
  if (!state.isGuest && launchParams.channel) {
    switchView('messages');
    openConversation('channel', launchParams.channel);
  } else if (!state.isGuest && launchParams.room) {
    switchView('messages');
    openConversation('room', launchParams.room);
  }

  applyDialogLabels();

  Tapp.ui.onLocaleChange(function (newLocale) {
    setLocale(newLocale);
    applyLabels();
    applyDialogLabels();
    renderConvList();
    renderChatHeader();
    renderMembers();
    renderFederationIdentity();
    if (state.currentView === 'feed') { renderFeedContent(); }
    else if (state.currentView === 'rings') { renderRingsSidebar(); if (state.activeRingId) renderRingDetail(); }
  });
}

// ==================== Entry ====================
if (window._TAPP_MODE === 'page' || window._TAPP_HAS_HTML) {
  Tapp.lifecycle.onReady(function () {
    init();
  });

  Tapp.lifecycle.onDestroy(function () {
    stopPolling();
    unsubscribeRealtime();
  });
}
`

const PAGE_MODULES: Record<string, string> = {
  'i18n.js': PAGE_MOD_I18N,
  'state.js': PAGE_MOD_STATE,
  'helpers.js': PAGE_MOD_HELPERS,
  'attachments.js': PAGE_MOD_ATTACHMENTS,
  'chat.js': PAGE_MOD_CHAT,
  'members.js': PAGE_MOD_MEMBERS,
  'api.js': PAGE_MOD_API,
  'views.js': PAGE_MOD_VIEWS,
  'events.js': PAGE_MOD_EVENTS,
  'index.js': PAGE_MOD_INDEX,
}

// ==================== Generated Monolith (from modules + inline LANG) ====================
function buildCoreCode(): string {
  const inlineLang = [
    '  // ==================== i18n ====================',
    `  var LANG = ${
      JSON.stringify(ARO_I18N, null, 2)
        .split('\n')
        .map((l, i) => (i === 0 ? l : `  ${l}`))
        .join('\n')
      };`,
    '',
    '  var lang = LANG.zh;',
    "  var currentLocale = 'zh';",
    '',
    '  function setLocale(locale) {',
    "    currentLocale = locale || 'zh';",
    "    var key = currentLocale.startsWith('zh') ? 'zh' : currentLocale.startsWith('ja') ? 'ja' : 'en';",
    '    lang = LANG[key] || LANG.en;',
    '  }',
  ].join('\n')

  const otherModules = [
    PAGE_MOD_STATE,
    PAGE_MOD_HELPERS,
    PAGE_MOD_ATTACHMENTS,
    PAGE_MOD_CHAT,
    PAGE_MOD_MEMBERS,
    PAGE_MOD_API,
    PAGE_MOD_VIEWS,
    PAGE_MOD_EVENTS,
    PAGE_MOD_INDEX,
  ]
    .map((m) =>
      m
        .split('\n')
        .map((l) => (l ? `  ${l}` : l))
        .join('\n'),
    )
    .join('\n\n')

  return [
    '(function () {',
    "  'use strict';",
    '',
    inlineLang,
    '',
    otherModules,
    '})();',
  ].join('\n')
}

const CORE_CODE = buildCoreCode()

// ==================== Manifest ====================
const manifest: TappManifest = {
  id: 'com.myriad.aro',
  name: 'Aro',
  version: '1.0.0',
  minSystemVersion: '0.2.1',
  description: '社交中心，统一管理消息、时间线、环网与个人资料',
  category: 'social',
  main: 'index.js',
  author: {
    name: 'Myriad Team',
    url: 'https://github.com/Myriad-Dreamin',
  },
  permissions: [
    'storage',
    'ui:notification',
    'ui:theme',
    'federation:read',
    'federation:write',
    'federation:message',
    'platform:read',
    'report:read',
    'tappList:read',
    'tappList:manage',
    'brew:read',
  ],
  iconSvg:
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"/></svg>',
  themeColor: '#6366f1',
  hasPage: true,
  // 声明真实后台需求：关窗后仍由 headless core 轮询新消息并通知。
  backgroundRequirements: ['notification'],
  settings: [
    { key: 'pollInterval', type: 'number', defaultValue: 15, label: '轮询间隔 (秒)', min: 5, max: 120, step: 5 },
    { key: 'notifyOnMessage', type: 'toggle', defaultValue: true, label: '新消息通知' },
  ],
  pageModules: [
    'i18n.js',
    'state.js',
    'helpers.js',
    'attachments.js',
    'chat.js',
    'members.js',
    'api.js',
    'views.js',
    'events.js',
    'index.js',
  ],
}

// ==================== Code Structure ====================
const codeStructure: TappCodeStructure = {
  core: CORE_CODE,
  styles: STYLES,
  pageHtml: PAGE_HTML,
  i18n: ARO_I18N,
  pageModules: PAGE_MODULES,
  pageModuleOrder: manifest.pageModules,
}

// ==================== Export ====================
export const aroTapp: ExampleTapp = {
  manifest,
  code: codeStructure,
  tags: [
    'official',
    'federation',
    'social',
    'messenger',
    'timeline',
    'rings',
    'profile',
  ],
}
