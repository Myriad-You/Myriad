/**
 * 缓存管理页面示例
 *
 * 演示如何集成后台任务系统
 */

import React from 'react'
import { CacheManagement } from '../components/CacheManagement'

export default function CacheManagementPage() {
  return (
    <div className="min-h-screen bg-gray-50 dark:bg-neutral-950 py-8 px-4">
      <div className="max-w-6xl mx-auto">
        <div className="mb-6">
          <h1 className="text-3xl font-bold text-gray-900 dark:text-gray-100 mb-2">
            缓存管理
          </h1>
          <p className="text-gray-600 dark:text-gray-400">
            管理平台数据缓存，触发后台处理任务
          </p>
        </div>

        <CacheManagement />
      </div>
    </div>
  )
}
