use anyhow::Result;
use notify_rust::{Notification, NotificationHandle};
pub struct Notifier {
    handle: Option<NotificationHandle>,
}
impl Notifier {
    pub fn new() -> Self {
        Self { handle: None }
    }
    pub fn send(&mut self, state: &str, body: &str) -> Result<()> {
        if let Some(mut handle) = self.handle.take() {
            handle.summary(&format!("OOM alerter: {state}"));
            handle.body(body);
            if handle.update().is_ok() {
                self.handle = Some(handle);
                return Ok(());
            }
            // A notification may have been dismissed externally.  Do not retain
            // the stale handle: retry as a new notification so later sends can
            // recover even when the update and the original notification race.
        }
        self.handle = Some(Self::show(state, body)?);
        Ok(())
    }

    fn show(state: &str, body: &str) -> Result<NotificationHandle> {
        Ok(Notification::new()
            .summary(&format!("OOM alerter: {state}"))
            .body(body)
            .appname("oom-alerter")
            .timeout(10_000)
            .show()?)
    }

    pub fn close(&mut self) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            // notify-rust 4.18's XDG handle close is intentionally infallible;
            // the library cannot report whether a server already dismissed it.
            handle.close();
        }
        Ok(())
    }
}
