//! Labeled synthetic input for hold/release lifecycle checks, never microphone
//! or ASR evidence. Used by headless tests and the opt-in native snapshot probe.
use gpui_kit::component::speech::{
    AudioFormat, AudioInput, AudioSink, RecognitionSession, SpeechError, SpeechRecognizer,
    SpeechSink,
};
use gpui_kit::{App, Subscription};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Default)]
pub struct Input {
    pub starts: Rc<Cell<usize>>,
    pub stops: Rc<Cell<usize>>,
}
impl AudioInput for Input {
    fn start(
        &self,
        _: AudioFormat,
        _: AudioSink,
        _: &mut App,
    ) -> Result<Subscription, SpeechError> {
        self.starts.set(self.starts.get() + 1);
        let stops = self.stops.clone();
        Ok(Subscription::new(move || stops.set(stops.get() + 1)))
    }
}
pub struct Recognizer;
struct Session(SpeechSink);
impl SpeechRecognizer for Recognizer {
    fn start(
        &self,
        sink: SpeechSink,
        _: &mut App,
    ) -> Result<Box<dyn RecognitionSession>, SpeechError> {
        Ok(Box::new(Session(sink)))
    }
}
impl RecognitionSession for Session {
    fn push_audio(&mut self, _: &[i16], _: &mut App) {}
    fn finish(&mut self, cx: &mut App) {
        self.0.phrase("spoken words", cx);
        self.0.finish(cx);
    }
}
