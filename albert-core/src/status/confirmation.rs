use super::StatusFeed;
use crate::confirmations::ConfirmationWait;
impl StatusFeed {
    pub(crate) fn wait_for_confirmation(&self, wait: ConfirmationWait) {
        *self.confirmation_wait.lock().unwrap() = Some(wait);
    }
    pub(crate) fn confirmation_wait(&self) -> Option<ConfirmationWait> {
        self.confirmation_wait.lock().unwrap().clone()
    }
}
