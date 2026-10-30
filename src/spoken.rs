//! Spoken output uses macOS's installed voices, independently of Parakeet input.
//! AppKit owns synthesis and playback. No model downloads or shell processes.
#![allow(deprecated)]
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub voice: Option<String>,
    pub words_per_minute: u16,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            voice: None,
            words_per_minute: 180,
        }
    }
}

pub struct Speaker {
    pub preferences: Preferences,
    #[cfg(target_os = "macos")]
    engine: Option<objc2::rc::Retained<objc2_app_kit::NSSpeechSynthesizer>>,
}
impl Speaker {
    pub fn new(mut preferences: Preferences) -> Self {
        preferences.words_per_minute = preferences.words_per_minute.clamp(80, 320);
        Self {
            preferences,
            #[cfg(target_os = "macos")]
            engine: None,
        }
    }
    pub fn voices() -> Vec<(String, String)> {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::{NSSpeechSynthesizer, NSVoiceName};
            use objc2_foundation::NSString;
            NSSpeechSynthesizer::availableVoices()
                .iter()
                .map(|voice| {
                    let attributes = NSSpeechSynthesizer::attributesForVoice(&voice);
                    // SAFETY: the AppKit constant is an immutable NSString key.
                    let label = attributes
                        .objectForKey(unsafe { NSVoiceName })
                        .and_then(|v| v.downcast_ref::<NSString>().map(ToString::to_string))
                        .unwrap_or_else(|| voice.to_string());
                    (voice.to_string(), label)
                })
                .collect()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Vec::new()
        }
    }
    pub fn speak(&mut self, text: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!text.trim().is_empty(), "Select some text to read aloud");
        anyhow::ensure!(
            text.len() <= 65_536,
            "Select less text to read aloud (maximum 64 KiB)"
        );
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::NSString;
            let engine = self
                .engine
                .get_or_insert_with(objc2_app_kit::NSSpeechSynthesizer::new);
            engine.stopSpeaking();
            let voice = self.preferences.voice.as_deref().map(NSString::from_str);
            anyhow::ensure!(
                engine.setVoice(voice.as_deref()),
                "That voice is no longer installed. Choose another in Voice settings."
            );
            engine.setRate(self.preferences.words_per_minute as f32);
            anyhow::ensure!(
                engine.startSpeakingString(&NSString::from_str(text)),
                "macOS could not start spoken output"
            );
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            anyhow::bail!("Spoken output is currently available on macOS")
        }
    }
    pub fn stop(&self) {
        #[cfg(target_os = "macos")]
        if let Some(engine) = &self.engine {
            engine.stopSpeaking();
        }
    }
    pub fn pause(&self) {
        #[cfg(target_os = "macos")]
        if let Some(engine) = &self.engine {
            engine.pauseSpeakingAtBoundary(objc2_app_kit::NSSpeechBoundary::WordBoundary);
        }
    }
    pub fn resume(&self) {
        #[cfg(target_os = "macos")]
        if let Some(engine) = &self.engine {
            engine.continueSpeaking();
        }
    }
}
impl Drop for Speaker {
    fn drop(&mut self) {
        self.stop();
    }
}
