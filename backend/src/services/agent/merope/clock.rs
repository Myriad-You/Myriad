//! What "now" is while her words are put together: the clock, except where
//! a turn is put together as of another moment (a simulated day, a real
//! turn answered again), so that the hour, how long ago things were, and
//! what has come due are as they were then. Scoped to the one task putting
//! the prompt together; nothing else sees it.

use chrono::{DateTime, Local, Utc};

tokio::task_local! {
    static AT: DateTime<Utc>;
}

/// Now, as this turn is put together.
pub fn now() -> DateTime<Utc> {
    AT.try_with(|at| *at).unwrap_or_else(|_| Utc::now())
}

/// [`now`], on the site's clock.
pub fn local_now() -> DateTime<Local> {
    now().with_timezone(&Local)
}

/// Put `work` together as of `at`.
#[cfg(test)]
pub(crate) async fn as_of<F: std::future::Future>(at: DateTime<Utc>, work: F) -> F::Output {
    AT.scope(at, work).await
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn a_turn_put_together_as_of_then_sees_then_and_nothing_else_does() {
        let then = chrono::Utc::now() - chrono::Duration::days(3);
        let seen = super::as_of(then, async { super::now() }).await;
        assert_eq!(seen, then);
        assert!(chrono::Utc::now() - super::now() < chrono::Duration::seconds(5));
    }
}
