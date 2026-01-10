/**
 * 资源加载监控组件（仅开发环境）
 * 显示当前资源加载队列状态
 */

import React, { useEffect, useState } from 'react'
import { clearLyricsCache, clearPlaylistCache } from '../utils/musicPlayer'
import { globalResourceLoader } from '../utils/resourceLoader'
import './ResourceLoaderMonitor.css'

const ResourceLoaderMonitor: React.FC = () => {
  const [stats, setStats] = useState(globalResourceLoader.getStats())
  const [isVisible, setIsVisible] = useState(false)
  const [showCacheCleared, setShowCacheCleared] = useState(false)

  useEffect(() => {
    const interval = setInterval(() => {
      setStats(globalResourceLoader.getStats())
    }, 1000)

    const handleKeyPress = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key === 'R') {
        setIsVisible(prev => !prev)
      }
    }

    window.addEventListener('keydown', handleKeyPress)

    return () => {
      clearInterval(interval)
      window.removeEventListener('keydown', handleKeyPress)
    }
  }, [])

  if (!isVisible) {
    return (
      <button className="toggle-monitor-btn" onClick={() => setIsVisible(true)}>
        📊 Ctrl+Shift+R
      </button>
    )
  }

  const hasActivity = stats.queued > 0 || stats.active > 0

  return (
    <div className="resource-monitor">
      <div className="monitor-header">
        <h3>📊 Resource Loader</h3>
        <button className="monitor-close-btn" onClick={() => setIsVisible(false)}>
          ✕
        </button>
      </div>

      <div className="monitor-stats">
        <div className={`stat-box queued ${hasActivity ? 'active' : ''}`}>
          <div className="stat-label">Queued</div>
          <div className="stat-value">{stats.queued}</div>
        </div>

        <div className={`stat-box active ${stats.active > 0 ? 'active' : ''}`}>
          <div className="stat-label">Active</div>
          <div className="stat-value">{stats.active}</div>
        </div>

        <div className="stat-box completed">
          <div className="stat-label">Completed</div>
          <div className="stat-value">{stats.completed}</div>
        </div>

        <div className={`stat-box failed ${stats.failed > 0 ? 'active' : ''}`}>
          <div className="stat-label">Failed</div>
          <div className="stat-value">{stats.failed}</div>
        </div>
      </div>

      <div className="monitor-section">
        <div className="section-title">Queue by Priority:</div>
        <div className="priority-list">
          {Object.entries(stats.queuedByPriority).map(([priority, count]) => (
            <div key={priority} className="priority-item">
              <span className="priority-name">{priority}</span>
              <span className={`priority-badge ${count > 0 ? 'active' : ''}`}>
                {count}
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="monitor-actions">
        <div className="section-title">Actions:</div>
        <div className="action-buttons">
          <button
            className="action-btn clear"
            onClick={() => {
              globalResourceLoader.clear()
              setStats(globalResourceLoader.getStats())
            }}
            disabled={!hasActivity}
          >
            🗑️ Clear Queue
          </button>
          <button
            className="action-btn clear"
            onClick={() => {
              globalResourceLoader.reset()
              setStats(globalResourceLoader.getStats())
            }}
          >
            🔄 Reset All
          </button>
          <button
            className="action-btn clear"
            onClick={() => {
              clearPlaylistCache()
              clearLyricsCache()
              setShowCacheCleared(true)
              setTimeout(() => setShowCacheCleared(false), 2000)
            }}
          >
            🗑️ Clear Cache
          </button>
        </div>
        {showCacheCleared && (
          <div className="section-title section-title-success">
            ✓ Cache cleared!
          </div>
        )}
      </div>
    </div>
  )
}

export default ResourceLoaderMonitor
