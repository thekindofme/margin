//! Loads a downloaded model into a transcription engine.
use crate::catalog::{Engine, ModelSpec};
use crate::models;
use anyhow::{anyhow, Result};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::{SpeechModel, TranscribeOptions};

pub struct Transcriber {
    pub model_id: String,
    engine: Box<dyn SpeechModel + Send>,
    language: Option<String>,
}

impl Transcriber {
    pub fn load(spec: &ModelSpec) -> Result<Self> {
        if !models::is_downloaded(spec) {
            return Err(anyhow!("{} is not downloaded", spec.name));
        }
        let path = models::local_path(spec);
        let map = |e: transcribe_rs::TranscribeError| anyhow!("loading {}: {e}", spec.name);
        let engine: Box<dyn SpeechModel + Send> = match spec.engine {
            Engine::Parakeet => Box::new(
                transcribe_rs::onnx::parakeet::ParakeetModel::load(&path, &Quantization::Int8).map_err(map)?,
            ),
            Engine::Moonshine => Box::new(
                transcribe_rs::onnx::moonshine::MoonshineModel::load(
                    &path,
                    transcribe_rs::onnx::moonshine::MoonshineVariant::Base,
                    &Quantization::default(),
                )
                .map_err(map)?,
            ),
            Engine::MoonshineStreaming => Box::new(
                transcribe_rs::onnx::moonshine::StreamingModel::load(&path, 0, &Quantization::default())
                    .map_err(map)?,
            ),
            Engine::SenseVoice => Box::new(
                transcribe_rs::onnx::sense_voice::SenseVoiceModel::load(&path, &Quantization::Int8)
                    .map_err(map)?,
            ),
            Engine::Canary => Box::new(
                transcribe_rs::onnx::canary::CanaryModel::load(&path, &Quantization::Int8).map_err(map)?,
            ),
            Engine::Cohere => Box::new(
                transcribe_rs::onnx::cohere::CohereModel::load(&path, &Quantization::Int8).map_err(map)?,
            ),
            Engine::Whisper => {
                Box::new(transcribe_rs::whisper_cpp::WhisperEngine::load(&path).map_err(map)?)
            }
        };
        Ok(Self { model_id: spec.id.to_string(), engine, language: None })
    }

    /// Language hint ("en", "de", ...). None = model default / auto-detect.
    pub fn set_language(&mut self, lang: Option<String>) {
        self.language = lang.filter(|l| !l.is_empty() && l != "auto");
    }

    /// Transcribe 16 kHz mono samples to text.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        let opts = TranscribeOptions { language: self.language.clone(), ..Default::default() };
        let r = self.engine.transcribe(samples, &opts).map_err(|e| anyhow!("{e}"))?;
        Ok(clean_text(&r.text))
    }
}

/// Collapse whitespace and drop common hallucinations on near-silent audio.
fn clean_text(t: &str) -> String {
    let s = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = s.to_lowercase();
    const JUNK: &[&str] = &["[blank_audio]", "(silence)", "[silence]", "[music]", "thanks for watching!"];
    if JUNK.iter().any(|j| lower == *j) {
        return String::new();
    }
    s
}
