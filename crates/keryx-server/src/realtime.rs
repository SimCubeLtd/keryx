//! Coalesced dashboard invalidation for connected browsers.
//!
//! The channel carries only a revision. Browsers fetch a fresh server-rendered
//! snapshot after each change, so skipped intermediate revisions never leave a
//! dashboard stale.

use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct DashboardUpdates {
    sender: watch::Sender<u64>,
}

impl DashboardUpdates {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(0);
        Self { sender }
    }

    /// Mark the current dashboard snapshot as stale.
    pub fn changed(&self) {
        self.sender
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Observe the latest revision. A new receiver starts with the current
    /// revision, which makes reconnects recover changes missed while offline.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_receive_the_latest_revision() {
        let updates = DashboardUpdates::new();
        let mut first = updates.subscribe();

        updates.changed();
        updates.changed();

        first.changed().await.expect("sender remains alive");
        assert_eq!(*first.borrow_and_update(), 2);
        assert_eq!(*updates.subscribe().borrow(), 2);
    }
}
