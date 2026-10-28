//! Latest-value session persistence on GPUI's background executor.
//!
//! Tokio's watch channel bounds pending snapshots to one. A single consumer
//! preserves write order; dropping the sender drains the final snapshot before
//! exit. Only successful writes are deduplicated, so a later save can retry.
use frame_profile::{ProfileStore, Session};
use futures::channel::oneshot;
use gpui_kit::{App, AppContext as _};
use tokio::sync::watch;

pub struct SessionWriter {
    sender: watch::Sender<Option<Session>>,
    done: oneshot::Receiver<()>,
}

impl SessionWriter {
    pub fn new(store: ProfileStore, cx: &mut App) -> Self {
        let (sender, mut receiver) = watch::channel::<Option<Session>>(None);
        let (finished, done) = oneshot::channel();
        cx.background_spawn(async move {
            let mut saved = None;
            while receiver.changed().await.is_ok() {
                let latest = receiver.borrow_and_update().clone();
                if latest == saved {
                    continue;
                }
                if let Some(session) = latest {
                    match store.save_session(&session) {
                        Ok(()) => saved = Some(session),
                        Err(error) => {
                            log::warn!("Could not preserve the browser session: {error:#}")
                        }
                    }
                }
            }
            let _ = finished.send(());
        })
        .detach();
        Self { sender, done }
    }

    pub fn save(&self, session: Session) {
        self.sender.send_replace(Some(session));
    }

    pub async fn finish(self) {
        let Self { sender, done } = self;
        drop(sender);
        let _ = done.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    async fn closing_writer_drains_latest_snapshot(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let store = ProfileStore::open(temp.path()).unwrap();
        let writer = cx.update(|cx| SessionWriter::new(store.clone(), cx));
        for active in 0..20 {
            writer.save(Session {
                active,
                ..Session::default()
            });
        }
        writer.finish().await;
        assert_eq!(store.session().unwrap().active, 19);
    }

    #[gpui_kit::test]
    async fn failed_write_can_retry_identical_snapshot(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let store = ProfileStore::open(temp.path()).unwrap();
        let connection = store.connect().unwrap();
        connection.execute_batch("CREATE TRIGGER refuse_session BEFORE INSERT ON session BEGIN SELECT RAISE(FAIL, 'fixture write refusal'); END;").unwrap();
        let writer = cx.update(|cx| SessionWriter::new(store.clone(), cx));
        let session = Session {
            active: 7,
            ..Session::default()
        };
        writer.save(session.clone());
        cx.run_until_parked();
        assert_eq!(store.session().unwrap().active, 0);
        connection
            .execute_batch("DROP TRIGGER refuse_session;")
            .unwrap();
        writer.save(session);
        writer.finish().await;
        assert_eq!(store.session().unwrap().active, 7);
    }
}
