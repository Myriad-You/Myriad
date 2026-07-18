//! Lane Queue 请求队列
//!
//! 每个用户会话一个 Lane（串行锁），全局并发上限。
//! 防止同一用户的并发请求竞态条件，控制系统整体负载。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, RwLock, Semaphore};

/// Lane Queue 全局管理器
///
/// 核心并发控制：
/// - 每个 lane（用户/会话）一个 Mutex，保证串行执行
/// - 全局 Semaphore 限制最大并发数
///
/// 使用方式：
/// ```ignore
/// let guard = queue.acquire("user:1").await?;
/// let result = agent.process(request).await;
/// drop(guard); // 释放锁，下一个请求可以执行
/// ```
pub struct LaneQueue {
    /// lane_key -> 串行锁
    lanes: RwLock<HashMap<String, Arc<Mutex<()>>>>,
    /// 全局并发上限
    global_semaphore: Arc<Semaphore>,
    /// 最大并发数（用于状态查询）
    max_concurrent: usize,
}

impl LaneQueue {
    /// 创建新的 LaneQueue
    ///
    /// `max_concurrent`: 全局最大并发执行数（推荐 4）
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            lanes: RwLock::new(HashMap::new()),
            global_semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
        }
    }

    /// 生成 lane key
    ///
    /// 同一用户的请求串行执行；不同用户可并行（受全局上限约束）。
    pub fn make_lane_key(user_id: i32, session_id: Option<&str>) -> String {
        match session_id {
            Some(sid) => format!("user:{}:session:{}", user_id, sid),
            None => format!("user:{}", user_id),
        }
    }

    /// 获取或创建 lane 的串行锁
    async fn get_or_create_lane(&self, lane_key: &str) -> Arc<Mutex<()>> {
        // 快路径：读锁检查
        {
            let lanes = self.lanes.read().await;
            if let Some(mutex) = lanes.get(lane_key) {
                return mutex.clone();
            }
        }
        // 慢路径：写锁创建
        let mut lanes = self.lanes.write().await;
        lanes
            .entry(lane_key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// 获取执行许可
    ///
    /// 1. 获取 lane 内串行锁（同一用户的请求排队等待）
    /// 2. 获取全局并发许可（控制系统整体负载）
    ///
    /// 返回的 `LaneGuard` drop 时自动释放两把锁。
    pub async fn acquire(&self, lane_key: &str) -> Result<LaneGuard, String> {
        let lane_mutex = self.get_or_create_lane(lane_key).await;

        // 先获取 lane 串行锁（同用户排队）
        let lane_lock = lane_mutex.lock_owned().await;

        tracing::debug!(
            lane = %lane_key,
            "[LaneQueue] Lane lock acquired, waiting for global permit"
        );

        // 再获取全局并发许可
        let permit = self
            .global_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "系统正在关闭".to_string())?;

        tracing::debug!(
            lane = %lane_key,
            available = self.global_semaphore.available_permits(),
            "[LaneQueue] Execution slot acquired"
        );

        Ok(LaneGuard {
            _lane_lock: lane_lock,
            _permit: permit,
            lane_key: lane_key.to_string(),
        })
    }

    /// 获取队列状态
    pub async fn get_status(&self) -> QueueStatus {
        let lanes = self.lanes.read().await;
        QueueStatus {
            total_lanes: lanes.len(),
            max_concurrent: self.max_concurrent,
            available_permits: self.global_semaphore.available_permits(),
        }
    }

    /// 清理空闲 Lane（请求驱动调用，防止 HashMap 无限增长）
    pub async fn cleanup_idle_lanes(&self) {
        let mut lanes = self.lanes.write().await;
        // Map 自身持有一个 Arc；执行中的 guard 和等待者各自还会持有一个。
        // 只有 strong_count == 1 时才能安全删除，否则后续请求可能创建第二把锁，
        // 破坏同 lane 串行执行保证。
        lanes.retain(|_, mutex| Arc::strong_count(mutex) > 1);
    }
}

/// Lane 执行守卫
///
/// 持有 lane 串行锁 + 全局并发许可。
/// Drop 时自动释放，允许下一个请求执行。
pub struct LaneGuard {
    _lane_lock: OwnedMutexGuard<()>,
    _permit: OwnedSemaphorePermit,
    /// Lane key（用于日志）
    pub lane_key: String,
}

impl Drop for LaneGuard {
    fn drop(&mut self) {
        tracing::debug!(
            lane = %self.lane_key,
            "[LaneQueue] Execution slot released"
        );
    }
}

/// 队列状态
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueueStatus {
    pub total_lanes: usize,
    pub max_concurrent: usize,
    pub available_permits: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cleanup_removes_only_idle_lanes() {
        let queue = LaneQueue::new(2);
        let active = queue.acquire("active").await.unwrap();
        let idle = queue.get_or_create_lane("idle").await;
        drop(idle);

        queue.cleanup_idle_lanes().await;

        let lanes = queue.lanes.read().await;
        assert!(lanes.contains_key("active"));
        assert!(!lanes.contains_key("idle"));
        drop(lanes);
        drop(active);

        queue.cleanup_idle_lanes().await;
        assert_eq!(queue.get_status().await.total_lanes, 0);
    }

    #[tokio::test]
    async fn cleanup_keeps_waiters_on_the_same_lane_lock() {
        let queue = Arc::new(LaneQueue::new(2));
        let active = queue.acquire("shared").await.unwrap();
        let waiter_lock = queue.get_or_create_lane("shared").await;
        let waiter = tokio::spawn(async move { waiter_lock.lock_owned().await });

        queue.cleanup_idle_lanes().await;
        assert_eq!(queue.get_status().await.total_lanes, 1);

        drop(active);
        let waiting_guard = waiter.await.unwrap();
        drop(waiting_guard);
        queue.cleanup_idle_lanes().await;
        assert_eq!(queue.get_status().await.total_lanes, 0);
    }

    /// P1-3: 等待用户输入时必须释放全局 permit，否则 max=N 个 waiting 会堵死队列
    #[tokio::test]
    async fn releasing_guard_returns_global_permit_for_other_lanes() {
        let queue = Arc::new(LaneQueue::new(1));
        let g1 = queue.acquire("lane-a").await.unwrap();
        assert_eq!(queue.get_status().await.available_permits, 0);

        // 模拟 wait-for-input：释放 guard 后其他 lane 应能立刻拿到许可
        drop(g1);
        assert_eq!(queue.get_status().await.available_permits, 1);

        let g2 = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            queue.acquire("lane-b"),
        )
        .await
        .expect("should not block after release")
        .unwrap();
        assert_eq!(queue.get_status().await.available_permits, 0);
        drop(g2);
        assert_eq!(queue.get_status().await.available_permits, 1);
    }
}
