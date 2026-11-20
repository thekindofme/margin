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

    /// Closest known speaker if at least `min` similar.
    pub fn nearest_above(&self, emb: &[f32], min: f32) -> Option<usize> {
        self.nearest(emb).filter(|&i| cosine(&self.centroids[i], emb) >= min)
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

/// One system-audio segment as the worker labelled it, kept until the end of the meeting.
#[derive(Debug, Clone)]
pub struct Labelled {
    pub label: String,
    /// Voice print, when the clip was long enough to get one.
    pub emb: Option<Vec<f32>>,
    pub secs: f32,
}

/// A speaker with at most this much talk time (and under [`FOLD_SHARE`] of all remote talk)
/// is a leftover of noisy early clips rather than a real person.
const FOLD_SECS: f32 = 10.0;
const FOLD_SHARE: f32 = 0.03;
/// Clips at least this long gave a trustworthy voice print when the speaker was created.
const LONG_CLIP_SECS: f32 = 2.5;
/// A small speaker that was created from a long clip is only folded when this similar.
const FOLD_MIN_SIM: f32 = 0.35;

/// End-of-meeting tidy-up of remote speakers: merge speakers whose voices turn out to be the
/// same person, and fold tiny leftovers (a few short, noisy clips) into the nearest real
/// speaker. Returns `(from, to)` label renames; chains are resolved.
pub fn consolidate(items: &[Labelled], threshold: f32) -> Vec<(String, String)> {
    struct C {
        label: String,
        sum: Vec<f32>,
        secs: f32,
        long: bool,
    }
    let mut cs: Vec<C> = Vec::new();
    for it in items {
        let i = match cs.iter().position(|c| c.label == it.label) {
            Some(i) => i,
            None => {
                cs.push(C { label: it.label.clone(), sum: vec![], secs: 0.0, long: false });
                cs.len() - 1
            }
        };
        let c = &mut cs[i];
        c.secs += it.secs;
        if let Some(e) = &it.emb {
            c.long |= it.secs >= LONG_CLIP_SECS;
            if c.sum.is_empty() {
                c.sum = vec![0.0; e.len()];
            }
            if c.sum.len() == e.len() {
                for (s, v) in c.sum.iter_mut().zip(e) {
                    *s += v * it.secs; // duration-weighted
                }
            }
        }
    }
    let total: f32 = cs.iter().map(|c| c.secs).sum();
    let sim = |a: &C, b: &C| if a.sum.is_empty() || b.sum.is_empty() { f32::NAN } else { cosine(&a.sum, &b.sum) };
    for i in 0..cs.len() {
        for j in i + 1..cs.len() {
            log::info!(
                "speakers {} ({:.0}s) vs {} ({:.0}s): similarity {:.2}",
                cs[i].label, cs[i].secs, cs[j].label, cs[j].secs, sim(&cs[i], &cs[j])
            );
        }
    }
    let mut renames: Vec<(String, String)> = Vec::new();
    let absorb = |cs: &mut Vec<C>, from: usize, to: usize, renames: &mut Vec<(String, String)>| {
        let f = cs.remove(from);
        let to = if to > from { to - 1 } else { to };
        let t = &mut cs[to];
        if t.sum.is_empty() {
            t.sum = f.sum.clone();
        } else if f.sum.len() == t.sum.len() {
            for (a, b) in t.sum.iter_mut().zip(&f.sum) {
                *a += b;
            }
        }
        t.secs += f.secs;
        t.long |= f.long;
        log::info!("speaker {} folded into {}", f.label, t.label);
        renames.push((f.label, t.label.clone()));
    };
    // 1. Same voice under two labels: merge the smaller into the larger.
    'merge: loop {
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                if sim(&cs[i], &cs[j]) >= threshold + 0.1 {
                    let (from, to) = if cs[i].secs < cs[j].secs { (i, j) } else { (j, i) };
                    absorb(&mut cs, from, to, &mut renames);
                    continue 'merge;
                }
            }
        }
        break;
    }
    // 2. Tiny leftovers go to the nearest real speaker.
    let small = |c: &C| c.secs < FOLD_SECS && c.secs < FOLD_SHARE * total;
    'fold: loop {
        if !cs.iter().any(|c| !small(c)) {
            break;
        }
        for i in 0..cs.len() {
            if !small(&cs[i]) {
                continue;
            }
            let best = (0..cs.len())
                .filter(|&j| j != i && !small(&cs[j]))
                .map(|j| (j, sim(&cs[i], &cs[j])))
                .max_by(|a, b| {
                    let (x, y) = (if a.1.is_nan() { -1.0 } else { a.1 }, if b.1.is_nan() { -1.0 } else { b.1 });
                    x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal).then(cs[a.0].secs.partial_cmp(&cs[b.0].secs).unwrap_or(std::cmp::Ordering::Equal))
                });
            if let Some((j, s)) = best {
                // Built only from short clips: its voice print was never reliable. Built from a
                // long clip: it might be a real, brief participant, so only fold a clear match.
                if !cs[i].long || s >= FOLD_MIN_SIM {
                    absorb(&mut cs, i, j, &mut renames);
                    continue 'fold;
                }
            }
        }
        break;
    }
    // Resolve chains (a -> b, b -> c  =>  a -> c).
    let resolved: Vec<(String, String)> = renames
        .iter()
        .map(|(f, t)| {
            let mut t = t.clone();
            while let Some((_, n)) = renames.iter().find(|(x, _)| *x == t) {
                t = n.clone();
            }
            (f.clone(), t)
        })
        .collect();
    resolved
}

#[cfg(test)]
mod consolidate_tests {
    use super::*;

    fn v(x: f32, y: f32) -> Option<Vec<f32>> {
        Some(vec![x, y, 0.1])
    }
    fn it(label: &str, emb: Option<Vec<f32>>, secs: f32) -> Labelled {
        Labelled { label: label.into(), emb, secs }
    }

    #[test]
    fn folds_noisy_early_seed_into_main_speaker() {
        // s1: two short noisy clips; s2: the real voice for 12 minutes.
        let mut items = vec![it("s1", v(0.2, 1.0), 1.9), it("s1", v(0.3, 1.0), 2.4)];
        for _ in 0..80 {
            items.push(it("s2", v(1.0, 0.1), 9.0));
        }
        assert_eq!(consolidate(&items, 0.5), vec![("s1".to_string(), "s2".to_string())]);
    }

    #[test]
    fn keeps_two_real_speakers_apart() {
        let mut items = vec![];
        for _ in 0..40 {
            items.push(it("s1", v(1.0, 0.0), 8.0));
            items.push(it("s2", v(0.0, 1.0), 8.0));
        }
        assert!(consolidate(&items, 0.5).is_empty());
    }

    #[test]
    fn brief_but_distinct_participant_survives() {
        // s3 spoke once for 6 s with a clear, different voice: not a leftover.
        let mut items = vec![it("s3", v(-1.0, 0.0), 6.0)];
        for _ in 0..60 {
            items.push(it("s1", v(0.0, 1.0), 9.0));
        }
        assert!(consolidate(&items, 0.5).is_empty());
    }

    #[test]
    fn same_voice_under_two_labels_merges() {
        let mut items = vec![];
        for _ in 0..20 {
            items.push(it("s1", v(1.0, 0.05), 8.0));
            items.push(it("s2", v(1.0, 0.0), 6.0));
        }
        assert_eq!(consolidate(&items, 0.5), vec![("s2".to_string(), "s1".to_string())]);
    }
}
