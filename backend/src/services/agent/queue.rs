//! Lane Queue 请求队列
//!
//! 每个用户会话一个 Lane（串行锁），全局并发上限。
//! 防止同一 lane 的并发请求竞态，控制系统整体负载。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
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
/// let guard = queue.acquire_timeout("user:1", timeout).await?;
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
    /// 当前正在等待许可的请求数（排队深度）
    waiting: AtomicUsize,
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
            waiting: AtomicUsize::new(0),
        }
    }

    /// 生成 lane key
    ///
    /// 有 session 时 `user:{id}:session:{sid}`，否则 `user:{id}`。同一 lane 串行；全局 Semaphore 限并发。
    pub fn make_lane_key(user_id: i32, session_id: Option<&str>) -> String {
        match session_id {
            Some(sid) if !sid.is_empty() => format!("user:{}:session:{}", user_id, sid),
            _ => format!("user:{}", user_id),
        }
    }

    /// Resolve the lane key for answer / resume paths so they match the original
    /// process/stream lane (session-scoped when the task belongs to a session).
    ///
    /// Priority:
    /// 1. Stored task lane_id (set from recipe.lane_key at execute time)
    /// 2. Explicit session id (from WAITING_TASKS or caller)
    /// 3. User-only lane (no session)
    pub fn resolve_answer_lane_key(
        user_id: i32,
        task_lane_id: Option<&str>,
        session_id: Option<&str>,
    ) -> String {
        if let Some(lane) = task_lane_id.map(str::trim).filter(|s| !s.is_empty()) {
            return lane.to_string();
        }
        Self::make_lane_key(user_id, session_id)
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

    /// Default max wait for a free execution slot (user-visible, avoids silent hang).
    pub const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 60;

    /// 获取执行许可（无限等待，仅在系统关闭时失败）。
    ///
    /// 用户路径请优先使用 [`Self::acquire_timeout`]。保留无超时入口供测试与内部调用。
    #[cfg(test)]
    pub async fn acquire(&self, lane_key: &str) -> Result<LaneGuard, String> {
        self.acquire_inner(lane_key, None).await
    }

    /// 获取执行许可，超时后返回可展示的错误（避免排队无限挂起）。
    pub async fn acquire_timeout(
        &self,
        lane_key: &str,
        timeout: std::time::Duration,
    ) -> Result<LaneGuard, String> {
        self.acquire_inner(lane_key, Some(timeout)).await
    }

    async fn acquire_inner(
        &self,
        lane_key: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<LaneGuard, String> {
        let lane_mutex = self.get_or_create_lane(lane_key).await;
        self.waiting.fetch_add(1, Ordering::Relaxed);

        let acquire_fut = async {
            // 先获取 lane 串行锁（按 `lane_key`，会话不同则不共享）
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
                .map_err(|_| "The system is shutting down".to_string())?;

            tracing::debug!(
                lane = %lane_key,
                available = self.global_semaphore.available_permits(),
                "[LaneQueue] Execution slot acquired"
            );

            Ok::<LaneGuard, String>(LaneGuard {
                _lane_lock: lane_lock,
                _permit: permit,
                lane_key: lane_key.to_string(),
            })
        };

        let result = match timeout {
            None => acquire_fut.await,
            Some(dur) => match tokio::time::timeout(dur, acquire_fut).await {
                Ok(result) => result,
                Err(_) => Err(format!(
                    "The system is busy. Waited more than {} seconds without a slot. Try again later.",
                    dur.as_secs().max(1)
                )),
            },
        };
        self.waiting.fetch_sub(1, Ordering::Relaxed);
        result
    }

    /// 获取队列状态
    pub async fn get_status(&self) -> QueueStatus {
        let lanes = self.lanes.read().await;
        QueueStatus {
            total_lanes: lanes.len(),
            max_concurrent: self.max_concurrent,
            available_permits: self.global_semaphore.available_permits(),
            waiting: self.waiting.load(Ordering::Relaxed),
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
    /// 正在排队等待许可的请求数
    pub waiting: usize,
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

    #[tokio::test]
    async fn acquire_timeout_returns_error_when_slots_exhausted() {
        let queue = Arc::new(LaneQueue::new(1));
        let held = queue.acquire("holder").await.unwrap();
        let err = match queue
            .acquire_timeout("waiter", std::time::Duration::from_millis(80))
            .await
        {
            Err(e) => e,
            Ok(_) => panic!("should time out while slot held"),
        };
        assert!(
            err.contains("busy") || err.contains("Waited more than"),
            "user-visible queue message, got: {err}"
        );
        drop(held);
    }

    #[test]
    fn resolve_answer_lane_key_prefers_stored_task_lane() {
        let key = LaneQueue::resolve_answer_lane_key(
            7,
            Some("user:7:session:sess_abc"),
            Some("other_session"),
        );
        assert_eq!(key, "user:7:session:sess_abc");
    }

    #[test]
    fn resolve_answer_lane_key_includes_session_when_no_task_lane() {
        let key = LaneQueue::resolve_answer_lane_key(3, None, Some("sess_xyz"));
        assert_eq!(key, "user:3:session:sess_xyz");
        // Empty session falls back to user-only
        assert_eq!(
            LaneQueue::resolve_answer_lane_key(3, Some(""), Some("")),
            "user:3"
        );
        assert_eq!(LaneQueue::make_lane_key(3, Some("sess_xyz")), key);
    }

    /// 等待用户输入时必须释放全局 permit，否则 max=N 个 waiting 会堵死队列。
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
