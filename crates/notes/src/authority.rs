//! Shared ownership of one authority, needed to retire externally deleted paths without a second store.
use std::sync::{Arc, Mutex, MutexGuard};
use theorem_ide_authority::RedCoreAuthority;
use theorem_ide_runtime::authority::*;

#[derive(Clone)]
pub(crate) struct SharedAuthority(pub Arc<Mutex<RedCoreAuthority>>);
impl SharedAuthority {
    fn lock(&self) -> Result<MutexGuard<'_, RedCoreAuthority>, AuthorityError> {
        self.0.lock().map_err(|_| AuthorityError::Storage {
            message: "vault authority lock poisoned".into(),
        })
    }
}
impl DocumentAuthority for SharedAuthority {
    fn describe(&self) -> AuthorityDescriptor {
        self.0
            .lock()
            .expect("vault authority lock poisoned")
            .describe()
    }
    fn resolve(&self, workspace: &str, path: &str) -> Result<Option<String>, AuthorityError> {
        self.lock()?.resolve(workspace, path)
    }
    fn head(&self, id: &str) -> Result<Option<AuthorityHead>, AuthorityError> {
        self.lock()?.head(id)
    }
    fn create(
        &mut self,
        workspace: &str,
        path: &str,
        text: &str,
        actor: &str,
        provenance: &str,
    ) -> Result<AuthorityHead, AuthorityError> {
        self.lock()?
            .create(workspace, path, text, actor, provenance)
    }
    fn commit(&mut self, commit: AuthorityCommit) -> Result<Vec<AuthorityHead>, AuthorityError> {
        let mut authority = self.lock()?;
        for change in &commit.changes {
            if let Some(head) = authority.head(&change.document_id)? {
                crate::vault::validate(&head.path, &change.text).map_err(|error| {
                    AuthorityError::Invalid {
                        reason: error.to_string(),
                    }
                })?;
            }
        }
        authority.commit(commit)
    }
    fn rename(
        &mut self,
        id: &str,
        expected_path: &str,
        new_path: &str,
        actor: &str,
    ) -> Result<AuthorityHead, AuthorityError> {
        self.lock()?.rename(id, expected_path, new_path, actor)
    }
    fn history(&self, id: &str, from: u64) -> Result<Vec<RevisionRecord>, AuthorityError> {
        self.lock()?.history(id, from)
    }
    fn transaction(&self, id: &str) -> Result<Option<TransactionRecord>, AuthorityError> {
        self.lock()?.transaction(id)
    }
    fn mark_undone(&mut self, id: &str, undone_by: &str) -> Result<(), AuthorityError> {
        self.lock()?.mark_undone(id, undone_by)
    }
    fn documents(&self, workspace: &str) -> Result<Vec<AuthorityHead>, AuthorityError> {
        self.lock()?.documents(workspace)
    }
    fn record_disk_hash(
        &mut self,
        id: &str,
        hash: &str,
        revision: &DiskRevision,
    ) -> Result<(), AuthorityError> {
        self.lock()?.record_disk_hash(id, hash, revision)
    }
    fn retire(
        &mut self,
        id: &str,
        expected_revision: u64,
        actor: &str,
    ) -> Result<(), AuthorityError> {
        self.lock()?.retire(id, expected_revision, actor)
    }
}
