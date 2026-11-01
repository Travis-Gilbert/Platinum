//! The checker and the personal profile, shared by every field.

use std::sync::{Arc, OnceLock, RwLock};

use theorem_text_check::{
    CheckOutcome, CheckRequest, CheckerError, EditRefusal, PersonalProfile, ProfileError,
    ProfileStore, Suggestion, TextChecker,
};

/// The checker and the person's profile.
///
/// Cheap to share (`Arc<Service>`) and safe to call from any thread. The
/// profile is read for every check and rewritten, atomically, when a word is
/// added or a rule switched off.
pub struct Service {
    checker: TextChecker,
    store: ProfileStore,
    profile: RwLock<PersonalProfile>,
    /// Why the profile file could not be read, if it could not. Checking goes
    /// on with an empty profile; changes are refused rather than written over
    /// a file nobody has looked at.
    problem: Option<String>,
}

impl Service {
    /// Open the service over a profile store.
    pub fn open(store: ProfileStore) -> Result<Self, CheckerError> {
        let checker = TextChecker::new()?;
        let (profile, problem) = match store.load() {
            Ok(profile) => (profile, None),
            Err(error) => (PersonalProfile::new(), Some(error.to_string())),
        };
        Ok(Self {
            checker,
            store,
            profile: RwLock::new(profile),
            problem,
        })
    }

    /// The service over the default profile: `$THEOREM_TEXT_CHECK_PROFILE`, or
    /// `Theorem/text-check/profile.json` in the platform config directory, so
    /// Platinum and Theorem's own apps share one personal dictionary.
    pub fn open_default() -> Result<Self, String> {
        let store = ProfileStore::at_default_location().map_err(|error| error.to_string())?;
        Self::open(store).map_err(|error| error.to_string())
    }

    /// Why the profile could not be read, if it could not.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Check one revision of one field with the current profile.
    #[must_use]
    pub fn check(&self, request: &CheckRequest) -> CheckOutcome {
        let profile = self.profile.read().unwrap_or_else(|e| e.into_inner());
        self.checker.check(request, &profile)
    }

    /// Every suggestion for one diagnostic, including the dictionary's
    /// replacements for a misspelling.
    pub fn suggestions(
        &self,
        outcome: &CheckOutcome,
        diagnostic: &str,
    ) -> Result<Vec<Suggestion>, EditRefusal> {
        self.checker.suggestions(outcome, diagnostic)
    }

    /// A copy of the profile as it is now.
    #[must_use]
    pub fn profile(&self) -> PersonalProfile {
        self.profile
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Add a word to the personal dictionary and save it.
    pub fn add_word(&self, word: &str) -> Result<(), ProfileError> {
        let saved = self.store.add_word(word)?;
        *self.profile.write().unwrap_or_else(|e| e.into_inner()) = saved;
        Ok(())
    }

    /// Stop reporting a rule, and save that.
    pub fn suppress_rule(&self, rule: &str) -> Result<(), ProfileError> {
        let saved = self.store.suppress_rule(rule)?;
        *self.profile.write().unwrap_or_else(|e| e.into_inner()) = saved;
        Ok(())
    }
}

static SHARED: OnceLock<Arc<Service>> = OnceLock::new();

/// Make `service` the one every field uses. The first call wins; later calls
/// get the service that is already installed.
pub fn install(service: Service) -> Arc<Service> {
    SHARED.get_or_init(|| Arc::new(service)).clone()
}

/// The installed service, opening the default one on first use. `None` when
/// neither can be had: checking is then off, never partial.
#[must_use]
pub fn shared() -> Option<Arc<Service>> {
    if let Some(service) = SHARED.get() {
        return Some(service.clone());
    }
    Service::open_default().ok().map(install)
}
