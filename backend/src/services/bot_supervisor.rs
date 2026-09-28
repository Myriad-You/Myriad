//! The connection supervisor every chat bot worker runs under.
//!
//! One loop owns what the four workers used to copy: read the credential
//! fingerprint, stay offline while disabled, stop retrying rejected
//! credentials until they change, cancel the session when they change,
//! back off transient failures and publish the phase. A platform supplies
//! only its session and how it shows each phase.

use std::future::Future;
use std::time::Duration;

use myriad_agent_rules::channel::{ConnectFailureKind, WorkerIntent};
use tokio::sync::watch;
use tracing::warn;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::DynamicConfig;

const POLL: Duration = Duration::from_secs(2);

/// What the supervisor reports; each platform maps it to its status enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorPhase {
    Offline,
    Rejected,
    Connecting,
    Reconnecting,
}

/// Outcome of one session: the state to resume with, or why it ended.
pub type SessionResult<R> = Result<Option<R>, (ConnectFailureKind, Option<R>)>;

pub trait BotWorker: Send + Sync + 'static {
    /// For logs.
    const NAME: &'static str;
    /// Everything that, when changed, requires a new session.
    type Fingerprint: Clone + PartialEq + Send + Sync + 'static;
    /// State carried from one session into the next reconnect (a gateway
    /// resume); `()` for workers that always start fresh.
    type Resume: Clone + Send + Sync + 'static;

    fn fingerprint(config: &DynamicConfig) -> Self::Fingerprint;
    fn intent(fingerprint: &Self::Fingerprint) -> WorkerIntent;
    fn publish(
        phase: SupervisorPhase,
        fingerprint: &Self::Fingerprint,
    ) -> impl Future<Output = ()> + Send;
    fn run_session(
        fingerprint: &Self::Fingerprint,
        resume: Option<Self::Resume>,
        cancel: watch::Receiver<bool>,
    ) -> impl Future<Output = SessionResult<Self::Resume>> + Send;
}

async fn current<W: BotWorker>() -> W::Fingerprint {
    W::fingerprint(&*GLOBAL_DYNAMIC_CONFIG.read().await)
}

pub async fn supervise<W: BotWorker>() {
    let mut last_permanent: Option<W::Fingerprint> = None;
    let mut resume: Option<W::Resume> = None;
    let mut reconnect_attempts: u32 = 0;
    loop {
        let fingerprint = current::<W>().await;

        if W::intent(&fingerprint) != WorkerIntent::Run {
            last_permanent = None;
            resume = None;
            reconnect_attempts = 0;
            W::publish(SupervisorPhase::Offline, &fingerprint).await;
            tokio::time::sleep(POLL).await;
            continue;
        }

        if last_permanent.as_ref() == Some(&fingerprint) {
            W::publish(SupervisorPhase::Rejected, &fingerprint).await;
            tokio::time::sleep(POLL).await;
            continue;
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let watched = fingerprint.clone();
        let watch_task = crate::services::channel_work::AbortTask(tokio::spawn(async move {
            loop {
                tokio::time::sleep(POLL).await;
                if current::<W>().await != watched {
                    let _ = cancel_tx.send(true);
                    break;
                }
            }
        }));

        W::publish(SupervisorPhase::Connecting, &fingerprint).await;
        let result = W::run_session(&fingerprint, resume.clone(), cancel_rx).await;
        drop(watch_task);
        match result {
            Ok(next) => {
                last_permanent = None;
                reconnect_attempts = 0;
                resume = next;
                W::publish(SupervisorPhase::Offline, &fingerprint).await;
            }
            Err((ConnectFailureKind::Permanent, _)) => {
                warn!(bot = W::NAME, "bot stopped: credentials rejected");
                last_permanent = Some(fingerprint.clone());
                reconnect_attempts = 0;
                resume = None;
                W::publish(SupervisorPhase::Rejected, &fingerprint).await;
            }
            Err((ConnectFailureKind::Transient, next)) => {
                reconnect_attempts = reconnect_attempts.saturating_add(1);
                let delay = crate::services::bot_ingress::reconnect_backoff(reconnect_attempts);
                warn!(
                    bot = W::NAME,
                    attempt = reconnect_attempts,
                    retry_in_secs = delay.as_secs(),
                    "bot transient failure; will reconnect"
                );
                resume = next;
                W::publish(SupervisorPhase::Reconnecting, &fingerprint).await;
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_bot_worker_reconnects_through_the_supervisor() {
        for (name, source) in [
            ("qq_bot", include_str!("qq_bot.rs")),
            ("feishu_bot", include_str!("feishu_bot.rs")),
            ("discord_bot", include_str!("discord_bot.rs")),
            ("telegram_bot", include_str!("telegram_bot.rs")),
            ("onebot_bot", include_str!("onebot_bot.rs")),
        ] {
            assert!(
                source.contains("supervise::<"),
                "{name} must run under bot_supervisor::supervise"
            );
            for private_loop in ["last_permanent", "reconnect_backoff", "fn run_loop"] {
                assert!(
                    !source.contains(private_loop),
                    "{name} keeps its own reconnect loop ({private_loop})"
                );
            }
        }
    }
}
