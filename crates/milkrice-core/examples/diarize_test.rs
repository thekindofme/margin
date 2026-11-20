use milkrice_core::diarize::{cosine, SpeakerClusterer, SpeakerEmbedder, DEFAULT_THRESHOLD};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).ok_or_else(|| anyhow::anyhow!("usage: diarize_test <dir with model.onnx + Voice_N.wav (N=0..2)> [voices...]"))?;
    let mut emb = SpeakerEmbedder::load(&std::path::Path::new(&dir).join("model.onnx"))?;
    let mut clips = vec![];
    let voices: Vec<String> = std::env::args().skip(2).collect();
    let voices = if voices.is_empty() { vec!["Samantha".into(), "Daniel".into(), "Karen".into()] } else { voices };
    let nv = voices.len();
    for v in &voices {
        for i in 0..3 {
            let name = format!("{v}_{i}");
            let mut r = hound::WavReader::open(format!("{dir}/{name}.wav"))?;
            let s: Vec<f32> = r.samples::<i16>().map(|x| x.map(|x| x as f32 / 32768.0)).collect::<Result<_, _>>()?;
            let t = Instant::now();
            let e = emb.embed(&s)?;
            println!("{name}: {:.1}s audio, dim {}, embed {:?}", s.len() as f32 / 16000.0, e.len(), t.elapsed());
            clips.push((name, e));
        }
    }
    print!("{:>12}", "");
    for (n, _) in &clips { print!("{n:>11}"); }
    println!();
    for (n, a) in &clips {
        print!("{n:>12}");
        for (_, b) in &clips { print!("{:>11.2}", cosine(a, b)); }
        println!();
    }
    let mut c = SpeakerClusterer::new(DEFAULT_THRESHOLD);
    println!("threshold {DEFAULT_THRESHOLD}");
    for k in 0..3 {
        for v in 0..nv {
            let (n, e) = &clips[v * 3 + k];
            println!("{n} -> speaker {}", c.assign(e));
        }
    }
    println!("speakers: {}", c.centroids.len());
    Ok(())
}
