//! Speaker recognition for the System (remote participants) stream: a speaker-embedding
//! ONNX model plus online cosine clustering. OWNER: diarize worker.
use anyhow::{anyhow, bail, Result};
use ort::{session::Session, value::Tensor};
use std::path::Path;

pub const EMBED_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34_LM.onnx";
pub const EMBED_MODEL_FILE: &str = "speaker-wespeaker-resnet34.onnx";

pub mod fbank;

/// Default cosine threshold for "same speaker" (see examples/diarize_test.rs).
pub const DEFAULT_THRESHOLD: f32 = 0.5;
const MIN_SECONDS: f32 = 0.4;
const MAX_WINDOW: usize = 10 * fbank::SAMPLE_RATE;

pub struct SpeakerEmbedder {
    session: Session,
    fbank: fbank::Fbank,
    input_name: String,
}

impl SpeakerEmbedder {
    pub fn load(path: &Path) -> Result<Self> {
        let session = Session::builder()
            .map_err(|e| anyhow!("ort builder: {e}"))?
            .with_intra_threads(2)
            .map_err(|e| anyhow!("ort threads: {e}"))?
            .commit_from_file(path)
            .map_err(|e| anyhow!("loading speaker model {}: {e}", path.display()))?;
        let input_name = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or_else(|| anyhow!("speaker model has no inputs"))?;
        Ok(Self { session, fbank: fbank::Fbank::new(), input_name })
    }

    /// 16 kHz mono samples (>= 0.4 s) -> L2-normalised embedding. Utterances longer
    /// than 10 s use their middle 10 s.
    pub fn embed(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        if (samples.len() as f32) < MIN_SECONDS * fbank::SAMPLE_RATE as f32 {
            bail!("utterance too short for speaker embedding ({} samples)", samples.len());
        }
        let samples = if samples.len() > MAX_WINDOW {
            let start = (samples.len() - MAX_WINDOW) / 2;
            &samples[start..start + MAX_WINDOW]
        } else {
            samples
        };
        let mut feats = self.fbank.compute(samples);
        let t = feats.len();
        if t == 0 {
            bail!("no fbank frames");
        }
        let mut mean = [0.0f32; fbank::NUM_BINS];
        for r in &feats {
            for (m, v) in mean.iter_mut().zip(r) {
                *m += v;
            }
        }
        mean.iter_mut().for_each(|m| *m /= t as f32);
        let mut flat = Vec::with_capacity(t * fbank::NUM_BINS);
        for r in feats.iter_mut() {
            for (v, m) in r.iter().zip(&mean) {
                flat.push(v - m);
            }
        }
        let input = Tensor::from_array(([1usize, t, fbank::NUM_BINS], flat))
            .map_err(|e| anyhow!("input tensor: {e}"))?;
        let outputs = self
            .session
            .run(ort::inputs![self.input_name.as_str() => input])
            .map_err(|e| anyhow!("speaker model run: {e}"))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow!("extract embedding: {e}"))?;
        let mut emb = data.to_vec();
        l2_normalize(&mut emb);
        Ok(emb)
    }
}

fn l2_normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-9 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na < 1e-9 || nb < 1e-9 { 0.0 } else { dot / (na * nb) }
}

/// Online agglomerative clustering of embeddings into speakers.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SpeakerClusterer {
    pub threshold: f32,
    pub centroids: Vec<Vec<f32>>,
    pub counts: Vec<u32>,
}

impl SpeakerClusterer {
    pub fn new(threshold: f32) -> Self {
        Self { threshold, centroids: vec![], counts: vec![] }
    }
    /// Returns 0-based speaker index (new index if no centroid is close enough).
    /// Closest known speaker, without creating a new one (None when nobody is known yet).
    pub fn nearest(&self, emb: &[f32]) -> Option<usize> {
        self.centroids
            .iter()
            .enumerate()
            .filter(|(_, c)| c.len() == emb.len())
            .map(|(i, c)| (i, cosine(c, emb)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
    }

    pub fn assign(&mut self, emb: &[f32]) -> usize {
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.centroids.iter().enumerate() {
            if c.len() != emb.len() {
                continue;
            }
            let s = cosine(c, emb);
            if best.is_none_or(|(_, bs)| s > bs) {
                best = Some((i, s));
            }
        }
        match best {
            Some((i, s)) if s >= self.threshold => {
                let n = self.counts[i] as f32;
                let c = &mut self.centroids[i];
                for (cv, ev) in c.iter_mut().zip(emb) {
                    *cv = *cv * n + ev;
                }
                l2_normalize(c);
                self.counts[i] += 1;
                i
            }
            _ => {
                let mut c = emb.to_vec();
                l2_normalize(&mut c);
                self.centroids.push(c);
                self.counts.push(1);
                self.centroids.len() - 1
            }
        }
    }

    /// Merge centroid pairs whose similarity is >= `threshold + 0.1`. Returns
    /// (removed, kept) index pairs; indices above a removed one shift down.
    pub fn merge_close(&mut self) -> Vec<(usize, usize)> {
        let mut merged = vec![];
        let t = self.threshold + 0.1;
        'outer: loop {
            for i in 0..self.centroids.len() {
                for j in i + 1..self.centroids.len() {
                    if cosine(&self.centroids[i], &self.centroids[j]) >= t {
                        let (ni, nj) = (self.counts[i] as f32, self.counts[j] as f32);
                        let cj = self.centroids.remove(j);
                        let cj_n = self.counts.remove(j);
                        let ci = &mut self.centroids[i];
                        for (a, b) in ci.iter_mut().zip(&cj) {
                            *a = *a * ni + b * nj;
                        }
                        l2_normalize(ci);
                        self.counts[i] += cj_n;
                        merged.push((j, i));
                        continue 'outer;
                    }
                }
            }
            break;
        }
        merged
    }
}
