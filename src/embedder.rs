//! EmbeddingGemma 2, in process, for every Platinum feature that embeds text.
//!
//! One copy of the model is loaded per process and shared through [`shared`]:
//! search embeds queries and visited pages with it, and later features reuse
//! the same copy instead of loading their own. The weights are the ONNX
//! Community conversion of Google's model, text component only (q8, about
//! 314 MB), pinned to one revision so the tokenizer always matches the graph.
//! They are kept under the user cache directory, since they can be downloaded
//! again.
//!
//! Interactive work comes first. [`TextEmbedder::embed`] (a search query) takes
//! the model as soon as it is free; [`EmbeddingGemma::embed_background`] (page
//! indexing) embeds one document at a time and steps aside whenever a query is
//! waiting, so a query waits for at most one document.
//!
//! The model runs on the GPU through ONNX Runtime's WebGPU provider (Dawn on
//! Metal), which embeds a query about twice as fast as the CPU and a full page
//! about four times as fast. Without a usable GPU it runs on the CPU. CoreML,
//! ONNX Runtime's default on macOS, is never used: it rejects the empty media
//! inputs every text embedding sends.
//!
//! SOURCING: `ort` runs the graph, `tokenizers` reads the model's own
//! `tokenizer.json`, and `hf-hub` fetches and caches the pinned files. fastembed
//! is not used: its text helper feeds `input_ids`, `attention_mask` and
//! `token_type_ids`, while this export also requires the image, video and audio
//! feature inputs (empty for text), so it cannot run unchanged through it.

use std::{
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, anyhow};
use futures::future::BoxFuture;
use ort::{
    environment::Environment,
    ep,
    session::{Session, builder::AutoDevicePolicy},
    value::{Shape, Tensor, TensorElementType, ValueType},
};
use rustyred_web::{EmbeddingError, TextEmbedder};
use tokenizers::{Tokenizer, TruncationParams};

const MODEL_REPO: &str = "onnx-community/embeddinggemma-2-ONNX";
/// The repository commit the graph and tokenizer are both read from.
const REVISION: &str = "daa72c51243991dfcaf9f9137d2c573d8f7790c0";
const MODEL_FILE: &str = "onnx/model_quantized.onnx";
const MODEL_DATA_FILE: &str = "onnx/model_quantized.onnx_data";
const TOKENIZER_FILE: &str = "tokenizer.json";
const DIMENSION: usize = 768;
/// Tokens embedded per text. The model reads 8,192; the opening of a page
/// carries its subject, and a shorter input keeps indexing light.
const MAX_TOKENS: usize = 512;
/// The graph's pooled, normalized output.
const OUTPUT: &str = "sentence_embedding";

static MODEL: OnceLock<Arc<EmbeddingGemma>> = OnceLock::new();

/// The loaded model, once [`load`] has finished.
pub fn shared() -> Option<Arc<EmbeddingGemma>> {
    MODEL.get().cloned()
}

/// Load the model, downloading its files the first time, and keep it for
/// [`shared`]. Blocks; call it off the main thread.
pub fn load() -> anyhow::Result<Arc<EmbeddingGemma>> {
    if let Some(model) = shared() {
        return Ok(model);
    }
    let cache = crate::brand::app_dir(
        &dirs::cache_dir().context("no cache directory for the embedding model")?,
    )
    .join("models");
    let model = Arc::new(EmbeddingGemma::open(&cache)?);
    Ok(MODEL.get_or_init(|| model).clone())
}

pub struct EmbeddingGemma {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    model_id: String,
    /// Interactive embeddings waiting for the session. Background work holds
    /// back while any are.
    interactive_waiting: AtomicUsize,
}

impl EmbeddingGemma {
    fn open(cache: &Path) -> anyhow::Result<Self> {
        let api = hf_hub::api::sync::ApiBuilder::new()
            .with_cache_dir(cache.to_path_buf())
            .with_progress(false)
            .build()
            .context("could not reach Hugging Face for the embedding model")?;
        let repo = api.repo(hf_hub::Repo::with_revision(
            MODEL_REPO.into(),
            hf_hub::RepoType::Model,
            REVISION.into(),
        ));
        let fetch = |file: &str| {
            repo.get(file)
                .with_context(|| format!("could not download {file} from {MODEL_REPO}"))
        };
        let model_path = fetch(MODEL_FILE)?;
        // The weights live beside the graph, which names them by file.
        fetch(MODEL_DATA_FILE)?;
        let mut tokenizer = Tokenizer::from_file(fetch(TOKENIZER_FILE)?)
            .map_err(|error| anyhow!("could not read the embedding tokenizer: {error}"))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKENS,
                ..Default::default()
            }))
            .map_err(|error| anyhow!("could not set the embedding tokenizer up: {error}"))?;

        let environment = ort::init()
            .with_name("frame")
            .build()
            .map_err(|error| anyhow!("could not start ONNX Runtime: {error}"))?;
        let (session, device) = match open_session(&environment, &model_path, true) {
            Ok(session) => (session, "the GPU"),
            Err(error) => {
                log::warn!("embedder: the GPU is unavailable, so the CPU embeds: {error:#}");
                let session = open_session(&environment, &model_path, false)
                    .map_err(|error| anyhow!("could not load the embedding model: {error}"))?;
                (session, "the CPU")
            }
        };
        log::info!(
            "embedder: {MODEL_REPO}@{REVISION} on {device}, inputs {:?}",
            session
                .inputs()
                .iter()
                .map(|input| format!("{}: {}", input.name(), input.dtype()))
                .collect::<Vec<_>>()
        );

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            model_id: format!("{MODEL_REPO}@{REVISION}:model_quantized"),
            interactive_waiting: AtomicUsize::new(0),
        })
    }

    /// Embed texts one at a time, stepping aside before each while a query is
    /// waiting for the model.
    pub fn embed_background(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        texts
            .iter()
            .map(|text| {
                while self.interactive_waiting.load(Ordering::Acquire) > 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let mut session = self.session()?;
                self.run(&mut session, text)
            })
            .collect()
    }

    fn embed_now(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.interactive_waiting.fetch_add(1, Ordering::AcqRel);
        let session = self.session();
        self.interactive_waiting.fetch_sub(1, Ordering::AcqRel);
        let mut session = session?;
        texts
            .iter()
            .map(|text| self.run(&mut session, text))
            .collect()
    }

    fn session(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Session>> {
        self.session
            .lock()
            .map_err(|_| anyhow!("the embedding model stopped after an earlier failure"))
    }

    fn run(&self, session: &mut Session, text: &str) -> anyhow::Result<Vec<f32>> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| anyhow!("could not tokenize text for embedding: {error}"))?;
        let tokens = |values: &[u32]| values.iter().map(|&v| i64::from(v)).collect::<Vec<_>>();
        let ids = tokens(encoding.get_ids());
        let mask = tokens(encoding.get_attention_mask());
        let length = ids.len();

        // Text-only inputs leave every media feature input empty: no rows, at
        // whatever width the graph declares.
        let mut inputs: Vec<(String, ort::session::SessionInputValue<'_>)> = Vec::new();
        for input in session.inputs() {
            let name = input.name().to_string();
            let value = match name.as_str() {
                "input_ids" => Tensor::from_array(([1, length], ids.clone()))?.into_dyn(),
                "attention_mask" => Tensor::from_array(([1, length], mask.clone()))?.into_dyn(),
                _ => match input.dtype() {
                    ValueType::Tensor {
                        ty: TensorElementType::Float32,
                        shape,
                        ..
                    } => {
                        let empty = Shape::new(shape.iter().map(|&dim| dim.max(0)));
                        Tensor::<f32>::from_array((empty, Vec::new()))?.into_dyn()
                    }
                    other => {
                        return Err(anyhow!(
                            "the embedding model asks for an unexpected input {name}: {other}"
                        ));
                    }
                },
            };
            inputs.push((name, value.into()));
        }

        let outputs = session.run(inputs)?;
        let (shape, values) = outputs
            .get(OUTPUT)
            .with_context(|| format!("the embedding model has no {OUTPUT} output"))?
            .try_extract_tensor::<f32>()?;
        if values.len() != DIMENSION {
            return Err(anyhow!(
                "the embedding model returned shape {shape:?}, expected one {DIMENSION}-wide vector"
            ));
        }
        Ok(values.to_vec())
    }
}

impl TextEmbedder for EmbeddingGemma {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn query_text(&self, query: &str) -> String {
        query_prompt(query)
    }

    fn document_text(&self, title: Option<&str>, body: &str) -> String {
        document_prompt(title, body)
    }

    fn embed<'a>(
        &'a self,
        inputs: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, EmbeddingError>> {
        Box::pin(async move {
            self.embed_now(inputs)
                .map_err(|error| EmbeddingError::Response {
                    reason: format!("{error:#}"),
                })
        })
    }
}

/// A session for the graph at `model`, on the GPU through WebGPU or on the CPU.
/// CPU-only operators run on half the cores either way, so indexing leaves
/// room for the page being read.
fn open_session(environment: &Environment, model: &Path, gpu: bool) -> ort::Result<Session> {
    let threads = std::thread::available_parallelism()
        .map(|cores| (cores.get() / 2).max(1))
        .unwrap_or(1);
    // PreferCPU keeps ONNX Runtime from registering CoreML by itself.
    let mut builder = Session::builder(environment)?
        .with_auto_device(AutoDevicePolicy::PreferCPU)?
        .with_intra_threads(threads)?;
    if gpu {
        builder =
            builder.with_execution_providers([ep::WebGPU::default().build().error_on_failure()])?;
    }
    builder.commit_from_file(model)
}

/// The model's web search query prefix.
fn query_prompt(query: &str) -> String {
    format!("task: search result | query: {query}")
}

/// The model's document prefix; `none` stands in for a missing title.
fn document_prompt(title: Option<&str>, body: &str) -> String {
    let title = title.map(str::trim).filter(|title| !title.is_empty());
    format!("title: {} | text: {body}", title.unwrap_or("none"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    /// The model card's search example: the Mars passage ranks first. Needs
    /// the model files, so it runs only when asked for.
    #[test]
    #[ignore = "downloads the 314 MB model on first run"]
    fn the_red_planet_query_finds_mars() {
        let model = load().unwrap();
        let query = model.query_text("Which planet is known as the Red Planet?");
        let documents = [
            "Venus is often called Earth's twin because of its similar size and proximity.",
            "Mars, known for its reddish appearance, is often referred to as the Red Planet.",
            "Jupiter, the largest planet in our solar system, has a prominent red spot.",
            "Saturn, famous for its rings, is sometimes mistaken for the Red Planet.",
        ]
        .map(|text| model.document_text(None, text));

        let query = model.embed_now(&[query]).unwrap().remove(0);
        let documents = model.embed_background(&documents).unwrap();

        let norm = dot(&query, &query).sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "query vector norm {norm}");
        let scores: Vec<f32> = documents.iter().map(|d| dot(&query, d)).collect();
        let best = (0..scores.len())
            .max_by(|&a, &b| scores[a].total_cmp(&scores[b]))
            .unwrap();
        assert_eq!(best, 1, "scores {scores:?}");
    }

    #[test]
    fn prompts_follow_the_model_card() {
        assert_eq!(query_prompt("tides"), "task: search result | query: tides");
        assert_eq!(document_prompt(None, "Body"), "title: none | text: Body");
        assert_eq!(
            document_prompt(Some("  "), "Body"),
            "title: none | text: Body"
        );
        assert_eq!(
            document_prompt(Some(" Aurora "), "Body"),
            "title: Aurora | text: Body"
        );
    }
}
