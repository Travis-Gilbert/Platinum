//! Shared ownership of one authority, needed to retire externally deleted paths without a second store.
use std::sync::{Arc, Mutex, MutexGuard};
use theorem_ide_authority::RedCoreAuthority;
use theorem_ide_runtime::authority::{
    AuthorityCommit, AuthorityDescriptor, AuthorityError, AuthorityHead, DiskRevision,
    DocumentAuthority, FormerBinding, RevisionRecord, TransactionRecord,
};

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

    fn pending_documents(&self, workspace: &str) -> Result<Vec<String>, AuthorityError> {
        self.lock()?.pending_documents(workspace)
    }

    fn projection_pending(&self, id: &str) -> Result<bool, AuthorityError> {
        self.lock()?.projection_pending(id)
    }

    fn former_documents(&self, workspace: &str, path: &str) -> Result<Vec<String>, AuthorityError> {
        self.lock()?.former_documents(workspace, path)
    }

    fn former_bindings(
        &self,
        workspace: &str,
        path: &str,
    ) -> Result<Vec<FormerBinding>, AuthorityError> {
        self.lock()?.former_bindings(workspace, path)
    }

    fn alias_candidates(&self, workspace: &str, path: &str) -> Result<Vec<String>, AuthorityError> {
        self.lock()?.alias_candidates(workspace, path)
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
                // A moved document is checked as the kind it becomes.
                let path = change
                    .path_move
                    .as_ref()
                    .map_or(&head.path, |m| &m.new_path);
                crate::vault::validate(path, &change.text).map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use theorem_ide_runtime::authority::{DocumentChange, PathMove, RevisionKind, diff_edits};

    #[test]
    fn a_moved_document_is_validated_as_the_kind_it_becomes() {
        let mut authority = SharedAuthority(Arc::new(Mutex::new(RedCoreAuthority::memory())));
        let head = authority
            .create("vault", "Plan.md", "# Plan", "test", "test")
            .unwrap();
        let move_to_canvas = |text: &str| AuthorityCommit {
            reapplies: None,
            reverts: None,
            operation_id: format!("move-{text}"),
            actor: "test".into(),
            provenance: "test".into(),
            transaction_id: None,
            changes: vec![DocumentChange {
                document_id: head.document_id.clone(),
                expected_revision: head.revision,
                new_revision: head.revision + 1,
                text: text.into(),
                edits: diff_edits(&head.text, text),
                kind: RevisionKind::Transaction,
                path_move: Some(PathMove {
                    expected_path: "Plan.md".into(),
                    new_path: "Plan.canvas".into(),
                }),
                projection: None,
            }],
        };
        assert!(matches!(
            authority.commit(move_to_canvas("# Plan")),
            Err(AuthorityError::Invalid { .. })
        ));
        let moved = authority.commit(move_to_canvas("{}")).unwrap();
        assert_eq!(moved[0].path, "Plan.canvas");
    }
}
