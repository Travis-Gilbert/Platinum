//! Local Parakeet recognition plugged into GPUI Kit's microphone/session owner.
//! One model per process; one bounded PCM queue per recording. No audio leaves
//! the machine. Focus/selection tokens make delayed results conditional.
use anyhow::{Context as _, anyhow};
use gpui_kit::component::speech::{RecognitionSession, SpeechError, SpeechRecognizer, SpeechSink};
use gpui_kit::{App, FocusHandle, Task, Window};
use parakeet_rs::{ExecutionConfig, ParakeetEOU, ParakeetEOUHandle};
use std::{
    ops::Range,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

static MODEL: OnceLock<ParakeetEOUHandle> = OnceLock::new();
static LOAD: Mutex<()> = Mutex::new(());
const REVISION: &str = "b53b9101fa87271041773b5036aeb8db9d810d40";
const CHUNK: usize = 2560;

pub fn ready() -> bool {
    MODEL.get().is_some()
}

/// Blocking initialization; the browser runs this off its UI executor before
/// opening the microphone. hf-hub owns download, cache, and revision resolution.
pub fn prepare() -> anyhow::Result<()> {
    let _guard = LOAD
        .lock()
        .map_err(|_| anyhow!("voice model loader unavailable"))?;
    if ready() {
        return Ok(());
    }
    let cache = crate::brand::app_dir(&dirs::cache_dir().context("no model cache directory")?)
        .join("models");
    let api = hf_hub::api::sync::ApiBuilder::new()
        .with_cache_dir(cache)
        .with_progress(false)
        .with_retries(1)
        .build()?;
    let repo = api.repo(hf_hub::Repo::with_revision(
        "altunenes/parakeet-rs".into(),
        hf_hub::RepoType::Model,
        REVISION.into(),
    ));
    let mut directory = None;
    for file in ["encoder.onnx", "decoder_joint.onnx", "tokenizer.json"] {
        let path = repo.get(&format!("realtime_eou_120m-v1-onnx/{file}"))?;
        directory = path.parent().map(|p| p.to_path_buf());
    }
    let config = ExecutionConfig::new().with_environment(crate::embedder::environment()?);
    let handle = ParakeetEOUHandle::from_pretrained(
        directory.context("missing model directory")?,
        Some(config),
    )?;
    let _ = MODEL.set(handle);
    Ok(())
}

// The pinned EOU tokenizer returns raw SentencePiece metaspace markers.
// Use tokenizers' decoder with no per-chunk leading-space removal so words
// spanning acoustic chunks concatenate correctly.
fn decoded_fragment(text: String) -> String {
    use tokenizers::{
        Decoder as _,
        decoders::metaspace::{Metaspace, PrependScheme},
    };
    Metaspace::new('▁', PrependScheme::Never, true)
        .decode(vec![text])
        .expect("metaspace decoding is infallible")
}

pub struct Recognizer;
impl SpeechRecognizer for Recognizer {
    fn is_available(&self, _: &App) -> bool {
        ready()
    }
    fn start(
        &self,
        sink: SpeechSink,
        cx: &mut App,
    ) -> Result<Box<dyn RecognitionSession>, SpeechError> {
        let handle = MODEL
            .get()
            .cloned()
            .ok_or_else(|| SpeechError::recognizer(anyhow!("Local voice model is not ready")))?;
        // 32 * 160 ms, at most 164 KiB PCM. Overflow fails visibly rather than
        // silently dropping words or allowing audio to accumulate unboundedly.
        let (tx, rx) = mpsc::sync_channel(32);
        let (results, receive) = mpsc::sync_channel(32);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        std::thread::Builder::new()
            .name("platinum-dictation".into())
            .spawn(move || {
                let mut model = ParakeetEOU::from_shared(&handle);
                // EOU needs one second of feature history before emitting. Prime
                // with silence so the beginning of the user's speech isn't skipped.
                for _ in 0..7 {
                    let _ = model.transcribe(&[0.; CHUNK], false);
                }
                let mut count = 0;
                while !cancel.load(Ordering::Acquire) {
                    let Ok(command) = rx.recv_timeout(Duration::from_secs(60)) else {
                        break;
                    };
                    let (audio, final_chunk) = match command {
                        Command::Audio(audio) => (audio, false),
                        Command::Finish(audio) => (audio, true),
                    };
                    count += audio.len();
                    if count > 16_000 * 60 * 5 {
                        let _ = results.try_send(Reply::Error(
                            "Dictation reached its five-minute limit; start another recording."
                                .into(),
                        ));
                        break;
                    }
                    match model.transcribe(&audio, false) {
                        Ok(text) => {
                            if results
                                .try_send(Reply::Text(decoded_fragment(text)))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = results.try_send(Reply::Error(error.to_string()));
                            break;
                        }
                    }
                    if final_chunk {
                        // Flush the acoustic context without resetting decoder state.
                        for _ in 0..7 {
                            match model.transcribe(&[0.; CHUNK], false) {
                                Ok(text) => {
                                    if results
                                        .try_send(Reply::Text(decoded_fragment(text)))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    let _ = results.try_send(Reply::Error(error.to_string()));
                                    return;
                                }
                            }
                        }
                        let _ = results.try_send(Reply::Done);
                        break;
                    }
                }
            })
            .map_err(SpeechError::recognizer)?;
        let report = sink.clone();
        let task = cx.spawn(async move |cx| {
            let mut started = false;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                loop {
                    match receive.try_recv() {
                        Ok(Reply::Text(text)) => {
                            let text = if started {
                                text
                            } else {
                                text.trim_start().to_owned()
                            };
                            started |= !text.is_empty();
                            cx.update(|cx| report.phrase(text, cx));
                        }
                        Ok(Reply::Done) => {
                            cx.update(|cx| report.finish(cx));
                            return;
                        }
                        Ok(Reply::Error(error)) => {
                            cx.update(|cx| {
                                report.error(SpeechError::recognizer(anyhow!(error)), cx)
                            });
                            return;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            cx.update(|cx| {
                                report.error(
                                    SpeechError::recognizer(anyhow!(
                                        "Dictation worker stopped before completion"
                                    )),
                                    cx,
                                )
                            });
                            return;
                        }
                    }
                }
            }
        });
        sink.ready(cx);
        Ok(Box::new(Session {
            tx,
            sink,
            cancelled,
            pending: Vec::with_capacity(CHUNK),
            _task: task,
        }))
    }
}
enum Command {
    Audio(Vec<f32>),
    Finish(Vec<f32>),
}
enum Reply {
    Text(String),
    Error(String),
    Done,
}
struct Session {
    tx: mpsc::SyncSender<Command>,
    sink: SpeechSink,
    cancelled: Arc<AtomicBool>,
    pending: Vec<f32>,
    _task: Task<()>,
}
impl Session {
    fn send(&self, command: Command, cx: &mut App) {
        if self.tx.try_send(command).is_err() {
            self.cancelled.store(true, Ordering::Release);
            self.sink.error(
                SpeechError::recognizer(anyhow!(
                    "Dictation couldn't keep up with the microphone. Please try again."
                )),
                cx,
            );
        }
    }
}
impl RecognitionSession for Session {
    fn push_audio(&mut self, samples: &[i16], cx: &mut App) {
        for sample in samples {
            self.pending.push(*sample as f32 / 32768.);
            if self.pending.len() == CHUNK {
                let audio = std::mem::replace(&mut self.pending, Vec::with_capacity(CHUNK));
                self.send(Command::Audio(audio), cx);
            }
        }
    }
    fn finish(&mut self, cx: &mut App) {
        let mut audio = std::mem::take(&mut self.pending);
        audio.resize(CHUNK, 0.);
        self.send(Command::Finish(audio), cx);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

/// Captured before recording: never resolve a destination from current focus
/// when recognition completes. Ordinary key/mouse/navigation events cancel it.
pub struct Destination {
    focus: FocusHandle,
    range: Range<usize>,
    original: String,
    marked: bool,
}
impl Destination {
    pub fn selected_text(window: &mut Window, cx: &mut App) -> Option<String> {
        Self::capture(window, cx).map(|target| target.original)
    }

    pub fn capture(window: &mut Window, cx: &mut App) -> Option<Self> {
        let focus = window.focused(cx)?;
        let (range, original) =
            window.with_focused_text_input(&focus, cx, |input, window, cx| {
                if input.marked_text_range(window, cx).is_some() {
                    return None;
                }
                let range = input.selected_text_range(false, window, cx)?.range;
                let original = input
                    .text_for_range(range.clone(), &mut None, window, cx)
                    .unwrap_or_default();
                Some((range, original))
            })??;
        Some(Self {
            focus,
            range,
            original,
            marked: false,
        })
    }
    pub fn apply(
        &mut self,
        text: &str,
        final_result: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if !window.is_window_active() {
            return false;
        }
        let applied = window
            .with_focused_text_input(&self.focus, cx, |input, window, cx| {
                if !self.marked
                    && input
                        .selected_text_range(false, window, cx)
                        .map(|s| s.range)
                        != Some(self.range.clone())
                {
                    return false;
                }
                if !self.marked
                    && input
                        .text_for_range(self.range.clone(), &mut None, window, cx)
                        .unwrap_or_default()
                        != self.original
                {
                    return false;
                }
                let range = if self.marked {
                    let Some(marked) = input.marked_text_range(window, cx) else {
                        return false;
                    };
                    Some(marked)
                } else {
                    Some(self.range.clone())
                };
                if final_result {
                    if self.marked {
                        // Restore the composition's original selection before
                        // the committed edit, so one Undo restores that text.
                        input.replace_and_mark_text_in_range(
                            range,
                            &self.original,
                            None,
                            window,
                            cx,
                        );
                        input.replace_text_in_range(Some(self.range.clone()), text, window, cx);
                    } else {
                        input.replace_text_in_range(range, text, window, cx);
                    }
                } else {
                    let end = text.encode_utf16().count();
                    input.replace_and_mark_text_in_range(range, text, Some(end..end), window, cx);
                }
                true
            })
            .unwrap_or(false);
        if applied {
            self.marked = !final_result;
        }
        applied
    }
    pub fn cancel(mut self, window: &mut Window, cx: &mut App) {
        if self.marked {
            let original = self.original.clone();
            self.apply(&original, true, window, cx);
        }
    }
}

/// Servo validates the exact document, element, focus sequence, text and
/// selection at admission. Until its rich-editor snapshot API exists, reject
/// unsupported website editors rather than guessing a field at completion.
pub enum Target {
    Native(Destination),
    Web(WebDestination),
}
impl Target {
    pub fn partial(&mut self, text: &str, window: &mut Window, cx: &mut App) -> bool {
        match self {
            Self::Native(target) => target.apply(text, false, window, cx),
            Self::Web(target) => target.send.send(WebCommand::Partial(text.into())).is_ok(),
        }
    }
    pub fn cancel(self, window: &mut Window, cx: &mut App) {
        if let Self::Native(target) = self {
            target.cancel(window, cx);
        }
        // Dropping the web sender requests rollback after any in-flight preedit.
    }
}

#[derive(Clone)]
enum WebCommand {
    Waiting,
    Partial(String),
    Final(String),
}
/// Latest-value mailbox: at most one edit in flight and one coalesced revision.
/// Each edit uses the exact context returned by Servo's previous admission.
pub struct WebDestination {
    send: tokio::sync::watch::Sender<WebCommand>,
}
impl WebDestination {
    pub fn new(
        view: servo::WebView,
        mut context: servo::NativeTextContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let (send, mut receive) = tokio::sync::watch::channel(WebCommand::Waiting);
        let original = String::from_utf16(
            &context.text.encode_utf16().collect::<Vec<_>>()
                [context.selection.0 as usize..context.selection.1 as usize],
        )
        .unwrap_or_default();
        let window = window.window_handle();
        cx.spawn(async move |cx| {
            loop {
                let command = if receive.changed().await.is_ok() {
                    receive.borrow_and_update().clone()
                } else {
                    if context.marked.is_none() {
                        return;
                    }
                    WebCommand::Final(original.clone())
                };
                let action = match &command {
                    WebCommand::Waiting => continue,
                    WebCommand::Partial(text) => {
                        let end = text.encode_utf16().count() as u32;
                        servo::NativeTextAction::Preedit {
                            range: None,
                            text: text.clone(),
                            selection: Some((end, end)),
                        }
                    }
                    WebCommand::Final(text) => servo::NativeTextAction::Replace {
                        range: None,
                        text: text.clone(),
                    },
                };
                let (reply, result) = futures::channel::oneshot::channel();
                view.native_text(
                    servo::NativeTextRequest::Edit {
                        expected: context,
                        action,
                    },
                    move |result| {
                        let _ = reply.send(result);
                    },
                );
                match native_result(result, cx.background_executor()).await {
                    Ok(Some(next)) => context = next,
                    result => {
                        use gpui_kit::component::{WindowExt as _, notification::Notification};
                        let message = match result {
                            Err(e) => e.to_string(),
                            _ => "The page editor closed during dictation".into(),
                        };
                        let _ = window.update(cx, |_, window, cx| {
                            window.push_notification(Notification::error(message), cx)
                        });
                        return;
                    }
                }
                if matches!(command, WebCommand::Final(_)) {
                    return;
                }
            }
        })
        .detach();
        Self { send }
    }
    pub fn finish(self, text: &str) {
        if !text.is_empty() {
            let _ = self.send.send(WebCommand::Final(text.into()));
        }
    }
}

pub async fn native_result(
    receive: futures::channel::oneshot::Receiver<servo::NativeTextResult>,
    executor: &gpui_kit::BackgroundExecutor,
) -> anyhow::Result<Option<servo::NativeTextContext>> {
    use futures::FutureExt as _;
    let result = receive.fuse();
    let timeout = executor.timer(Duration::from_secs(3)).fuse();
    futures::pin_mut!(result, timeout);
    futures::select! {
        result = result => result.map_err(|_| anyhow!("Page closed"))?.map_err(|error| {
            use servo::NativeTextError::*;
            anyhow!(match error {
                StaleContext | DocumentUnavailable => "Dictation stopped because the page or text field changed.",
                Unsupported => "This field does not support dictation. Try a plain text field.",
                InvalidRange | EditRefused => "This editor could not insert the dictated text. Select the field and try again.",
                TooLarge => "This field is too large for dictation. Try a smaller text field.",
            })
        }),
        _ = timeout => Err(anyhow!("Page editor did not respond")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::speech::{
        AudioFormat, AudioInput, AudioSink, SpeechEvent, SpeechState, SpeechStatus,
    };
    use gpui_kit::{AppContext as _, Subscription, TestAppContext};
    use std::{cell::RefCell, rc::Rc};

    struct UnfinishedRecognizer;
    impl SpeechRecognizer for UnfinishedRecognizer {
        fn start(
            &self,
            sink: SpeechSink,
            cx: &mut App,
        ) -> Result<Box<dyn RecognitionSession>, SpeechError> {
            sink.hypothesis("unfinished hypothesis", cx);
            Ok(Box::new(Self))
        }
    }
    impl RecognitionSession for UnfinishedRecognizer {
        fn push_audio(&mut self, _: &[i16], _: &mut App) {}
        fn finish(&mut self, _: &mut App) {}
    }
    impl AudioInput for UnfinishedRecognizer {
        fn start(
            &self,
            _: AudioFormat,
            _: AudioSink,
            _: &mut App,
        ) -> Result<Subscription, SpeechError> {
            Ok(Subscription::new(|| {}))
        }
    }
    #[gpui_kit::test]
    fn unfinished_recognition_errors_instead_of_committing_partial_text(cx: &mut TestAppContext) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let state = cx.update(|cx| {
            cx.new(|cx| {
                SpeechState::new(cx)
                    .recognizer(UnfinishedRecognizer)
                    .input(UnfinishedRecognizer)
                    .require_final_result(true)
                    .stop_timeout(Duration::from_secs(1))
            })
        });
        let _subscription = cx.update(|cx| {
            let events = events.clone();
            cx.subscribe(&state, move |_, event: &SpeechEvent, _| {
                events.borrow_mut().push(event.clone())
            })
        });
        state.update(cx, |state, cx| state.start(cx));
        cx.run_until_parked();
        state.update(cx, |state, cx| state.stop(cx));
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| state.read(cx).status()), SpeechStatus::Idle);
        assert!(matches!(
            events.borrow().last(),
            Some(SpeechEvent::Error(_))
        ));
        assert!(
            !events
                .borrow()
                .iter()
                .any(|event| matches!(event, SpeechEvent::Final(_)))
        );
    }

    #[test]
    #[ignore = "downloads the pinned model and requires PLATINUM_VOICE_FIXTURE (16kHz mono WAV)"]
    fn pinned_parakeet_recognizes_a_real_audio_fixture() {
        prepare().unwrap();
        let path = std::env::var("PLATINUM_VOICE_FIXTURE").expect("fixture path");
        let reader = hound::WavReader::open(path).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().channels, 1);
        let audio: Vec<f32> = reader
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.)
            .collect();
        let mut model = ParakeetEOU::from_shared(MODEL.get().unwrap());
        for _ in 0..7 {
            model.transcribe(&[0.; CHUNK], false).unwrap();
        }
        let mut transcript = String::new();
        for chunk in audio.chunks(CHUNK) {
            let mut padded = chunk.to_vec();
            padded.resize(CHUNK, 0.);
            transcript.push_str(&model.transcribe(&padded, false).unwrap());
        }
        for _ in 0..7 {
            transcript.push_str(&model.transcribe(&[0.; CHUNK], false).unwrap());
        }
        let transcript = decoded_fragment(transcript);
        assert!(!transcript.contains('▁'));
        eprintln!("Local fixture transcript: {transcript}");
        let lower = transcript.to_lowercase();
        assert!(
            lower.contains("river") && lower.contains("mountain"),
            "unexpected fixture recognition: {transcript}"
        );
    }
}

#[cfg(test)]
mod destination_tests {
    use super::*;
    use gpui_kit::{AppContext as _, Focusable as _, TestAppContext, VisualTestContext, px, size};
    #[gpui_kit::test]
    fn composition_commits_once_undo_restores_and_changed_focus_refuses(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            zorite_editor::bind_keys(cx);
        });
        let mut field = None;
        let handle = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            let editor =
                cx.new(|cx| zorite_editor::EditorState::new(window, cx).with_text("before after"));
            editor.update(cx, |e, cx| e.set_cursor(7, cx));
            field = Some(editor.clone());
            window.activate_window();
            window.focus(&editor.focus_handle(cx), cx);
            gpui_kit::component::Root::new(editor, window, cx)
        });
        let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
        let field = field.unwrap();
        cx.run_until_parked();
        let mut destination = cx.update(|window, cx| Destination::capture(window, cx).unwrap());
        cx.update(|window, cx| {
            assert!(destination.apply("spoken", false, window, cx));
            assert!(destination.apply("spoken words ", false, window, cx));
            assert!(destination.apply("spoken words ", true, window, cx));
            assert_eq!(field.read(cx).text(), "before spoken words after");
            window.dispatch_action(Box::new(zorite_editor::Undo), cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(field.read(cx).text(), "before after"));
        let mut stale = cx.update(|window, cx| Destination::capture(window, cx).unwrap());
        let other = cx.update(|window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            focus
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(other.is_focused(window));
            assert!(!stale.apply("wrong field", true, window, cx));
            assert_eq!(field.read(cx).text(), "before after");
        });
    }
}
