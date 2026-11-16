//! The open-weights speech-to-text models Margin can download and run locally.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    Canary,
    Cohere,
    Whisper,
}

/// One file of a model, pinned by size and SHA-256 (downloads are verified before use).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelFile {
    pub url: &'static str,
    /// File name inside the model directory (or the file itself for single-file models).
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub tagline: &'static str,
    pub engine: Engine,
    /// Legacy single download (a .tar.gz or .bin); used only when `files` is empty.
    pub url: &'static str,
    /// Files fetched from the model's upstream source (preferred).
    #[serde(skip)]
    pub files: &'static [ModelFile],
    /// Directory (for archives) or file name (for Whisper .bin) inside the models dir.
    pub local_name: &'static str,
    pub size_mb: u64,
    pub languages: &'static [&'static str],
    /// 0..1 — relative, for the UI bars.
    pub accuracy: f32,
    pub speed: f32,
    pub translate: bool,
    /// Low-latency model well suited to live captions.
    pub live: bool,
    pub recommended: bool,
}

impl ModelSpec {
    pub fn is_archive(&self) -> bool {
        self.files.is_empty() && self.url.ends_with(".tar.gz")
    }
    /// True when the model lives in a directory (everything except single-file Whisper).
    pub fn is_dir(&self) -> bool {
        self.engine != Engine::Whisper
    }
    pub fn language_label(&self) -> String {
        match self.languages.len() {
            0 => "Any language".into(),
            1 if self.languages[0] == "en" => "English only".into(),
            n => format!("{n} languages"),
        }
    }
}

const EUROPEAN_25: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
];
#[allow(dead_code)]
const COHERE_14: &[&str] = &[
    "en", "fr", "de", "it", "es", "pt", "el", "nl", "pl", "zh", "ja", "ko", "vi", "ar",
];
const WHISPER_99: &[&str] = &[
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it",
    "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur",
    "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
    "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si",
    "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo",
    "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha",
    "ba", "jw", "su",
];

const F_PARAKEET_V3: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx", name: "encoder-model.int8.onnx", size: 652183999, sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx", name: "decoder_joint-model.int8.onnx", size: 18202004, sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx", name: "nemo128.onnx", size: 139764, sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt", name: "vocab.txt", size: 93939, sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d" },
];
const F_PARAKEET_V2: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/encoder-model.int8.onnx", name: "encoder-model.int8.onnx", size: 652184014, sha256: "3e0581fda6ab843888b51e56d7ee78b6d5bc3237ec113af1f732d1d5286aa155" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/decoder_joint-model.int8.onnx", name: "decoder_joint-model.int8.onnx", size: 8998286, sha256: "a449f49acd68979d418651dd2dcb737cc0f1bf0225e009e29ee326354edbf7d3" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/nemo128.onnx", name: "nemo128.onnx", size: 139764, sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85/vocab.txt", name: "vocab.txt", size: 9384, sha256: "ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d" },
];
const F_CANARY_180M: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/encoder-model.int8.onnx", name: "encoder-model.int8.onnx", size: 133710896, sha256: "996d1c89e6cbc891a7c88bf410884c178ffa474f7b13084522ac74a5e144cc81" },
    ModelFile { url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/decoder-model.int8.onnx", name: "decoder-model.int8.onnx", size: 79520211, sha256: "9dd9c447872088c912e916d73751f9621a54085d5bc46788454fe904db51a914" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx", name: "nemo128.onnx", size: 139764, sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f" },
    ModelFile { url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/vocab.txt", name: "vocab.txt", size: 53555, sha256: "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77" },
];
const F_CANARY_1B: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/encoder-model.int8.onnx", name: "encoder-model.int8.onnx", size: 859078138, sha256: "6d96e9945898e5ace48f4efecd459ca1df81859730be27b8af6b197639403ee1" },
    ModelFile { url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/decoder-model.int8.onnx", name: "decoder-model.int8.onnx", size: 170040374, sha256: "52d83aa7aad41fbbe4f9dfcd341d784735a6eb4c6eb0d3290fc27a0d8ac39abf" },
    ModelFile { url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx", name: "nemo128.onnx", size: 139764, sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f" },
    ModelFile { url: "https://huggingface.co/istupakov/canary-1b-v2-onnx/resolve/5ebc1520cef7b6b318b3526ad17adbfe00bc1bfc/vocab.txt", name: "vocab.txt", size: 208022, sha256: "2c9efe6104fd29522ea27ce0e3aef5d37c690af4e5a4232e643e23ca403ffea3" },
];
const F_WHISPER_MEDIUM: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-medium-q5_0.bin", name: "ggml-medium-q5_0.bin", size: 539212467, sha256: "19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f" },
];
const F_WHISPER_TURBO: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo.bin", name: "ggml-large-v3-turbo.bin", size: 1624555275, sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69" },
];
const F_WHISPER_SMALL: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin", name: "ggml-small.bin", size: 487601967, sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b" },
];
const F_MOONSHINE_BASE: &[ModelFile] = &[
    ModelFile { url: "https://huggingface.co/moonshine-ai/moonshine/resolve/48b4e427b587bcf67797a5be706d6ddc4a298149/onnx/merged/base/quantized_4bit/encoder_model.onnx", name: "encoder_model.onnx", size: 31027744, sha256: "e6645050b45f5281214fd2788f3d6226cc94149a55c9d503f8b880acb99826a6" },
    ModelFile { url: "https://huggingface.co/moonshine-ai/moonshine/resolve/48b4e427b587bcf67797a5be706d6ddc4a298149/onnx/merged/base/quantized_4bit/decoder_model_merged.onnx", name: "decoder_model_merged.onnx", size: 42427308, sha256: "29bd47951f72e593e0d09aecb81905d20b7892d4574bf23d16e79c0cb2a91e82" },
    ModelFile { url: "https://huggingface.co/moonshine-ai/moonshine/resolve/48b4e427b587bcf67797a5be706d6ddc4a298149/onnx/merged/base/float/tokenizer.json", name: "tokenizer.json", size: 3761751, sha256: "ad3a2ceb0e84e4da57451d86fa337f8116dcff5f5d106434f8aa0b0de89718b9" },
];
const F_MOONSHINE_MEDIUM: &[ModelFile] = &[
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/frontend.ort", name: "frontend.ort", size: 47467256, sha256: "378fe8a5d7090a1b9ab88bbb1fc95bde010cdd64ec23419350d2d23c675636e9" },
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/encoder.ort", name: "encoder.ort", size: 94202872, sha256: "a5f11167a62eef61787fe8410453257d6ddb8eba90af461a9604e5f2e93d5322" },
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/adapter.ort", name: "adapter.ort", size: 3647712, sha256: "16307442b7f4229f2f1511fc51b545cec9616e55872c588f3a297bbc6f4762ea" },
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/cross_kv.ort", name: "cross_kv.ort", size: 11544952, sha256: "354b9a955caeb768b528f447f0a36ce4b850ca7b4531900165df304d97904fba" },
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/decoder_kv.ort", name: "decoder_kv.ort", size: 146216448, sha256: "fa67aa87521247f5bf44d3e44d4e4978e58c1f114249c3c6909c882624056715" },
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/streaming_config.json", name: "streaming_config.json", size: 513, sha256: "28e83b7a28e91472692a035e0dae3116422ae43aeb2bef5ed822c44ce89b88af" },
    ModelFile { url: "https://download.moonshine.ai/model/medium-streaming-en/quantized/tokenizer.bin", name: "tokenizer.bin", size: 249974, sha256: "6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d" },
];

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "parakeet-v3",
        name: "Parakeet TDT 0.6B v3",
        tagline: "Fast, accurate notes in 25 European languages",
        engine: Engine::Parakeet,
        url: "https://blob.handy.computer/parakeet-v3-int8.tar.gz",
        files: F_PARAKEET_V3,
        local_name: "parakeet-tdt-0.6b-v3-int8",
        size_mb: 671,
        languages: EUROPEAN_25,
        accuracy: 0.82,
        speed: 0.86,
        translate: false,
        live: true,
        recommended: true,
    },
    ModelSpec {
        id: "parakeet-v2",
        name: "Parakeet TDT 0.6B v2",
        tagline: "The sharpest English model for everyday meetings",
        engine: Engine::Parakeet,
        url: "https://blob.handy.computer/parakeet-v2-int8.tar.gz",
        files: F_PARAKEET_V2,
        local_name: "parakeet-tdt-0.6b-v2-int8",
        size_mb: 661,
        languages: &["en"],
        accuracy: 0.86,
        speed: 0.86,
        translate: false,
        live: true,
        recommended: true,
    },
    ModelSpec {
        id: "canary-180m-flash",
        name: "Canary 180M Flash",
        tagline: "Tiny and instant, runs well on any hardware",
        engine: Engine::Canary,
        url: "https://blob.handy.computer/canary-180m-flash.tar.gz",
        files: F_CANARY_180M,
        local_name: "canary-180m-flash",
        size_mb: 213,
        languages: &["en", "de", "es", "fr"],
        accuracy: 0.75,
        speed: 0.92,
        translate: true,
        live: true,
        recommended: false,
    },
    ModelSpec {
        id: "canary-1b-v2",
        name: "Canary 1B v2",
        tagline: "Accurate multilingual with translation",
        engine: Engine::Canary,
        url: "https://blob.handy.computer/canary-1b-v2.tar.gz",
        files: F_CANARY_1B,
        local_name: "canary-1b-v2",
        size_mb: 1029,
        languages: EUROPEAN_25,
        accuracy: 0.85,
        speed: 0.7,
        translate: true,
        live: true,
        recommended: false,
    },
    ModelSpec {
        id: "whisper-medium",
        name: "Whisper Medium",
        tagline: "Broadest language coverage, a little slower",
        engine: Engine::Whisper,
        url: "https://blob.handy.computer/whisper-medium-q4_1.bin",
        files: F_WHISPER_MEDIUM,
        local_name: "ggml-medium-q5_0.bin",
        size_mb: 539,
        languages: WHISPER_99,
        accuracy: 0.78,
        speed: 0.45,
        translate: true,
        live: false,
        recommended: false,
    },
    ModelSpec {
        id: "whisper-turbo",
        name: "Whisper Large v3 Turbo",
        tagline: "Big-model quality across 99 languages, GPU accelerated",
        engine: Engine::Whisper,
        url: "https://blob.handy.computer/ggml-large-v3-turbo.bin",
        files: F_WHISPER_TURBO,
        local_name: "ggml-large-v3-turbo.bin",
        size_mb: 1625,
        languages: WHISPER_99,
        accuracy: 0.84,
        speed: 0.4,
        translate: true,
        live: false,
        recommended: false,
    },
    ModelSpec {
        id: "whisper-small",
        name: "Whisper Small",
        tagline: "Light multilingual model for older machines",
        engine: Engine::Whisper,
        url: "https://blob.handy.computer/ggml-small.bin",
        files: F_WHISPER_SMALL,
        local_name: "ggml-small.bin",
        size_mb: 488,
        languages: WHISPER_99,
        accuracy: 0.6,
        speed: 0.8,
        translate: true,
        live: false,
        recommended: false,
    },
    ModelSpec {
        id: "moonshine-medium",
        name: "Moonshine v2 Medium",
        tagline: "Streaming English, made for live captions",
        engine: Engine::MoonshineStreaming,
        url: "https://blob.handy.computer/moonshine-medium-streaming-en.tar.gz",
        files: F_MOONSHINE_MEDIUM,
        local_name: "moonshine-medium-streaming-en",
        size_mb: 303,
        languages: &["en"],
        accuracy: 0.74,
        speed: 0.82,
        translate: false,
        live: true,
        recommended: false,
    },
    ModelSpec {
        id: "moonshine-base",
        name: "Moonshine Base",
        tagline: "Very small and quick English model",
        engine: Engine::Moonshine,
        url: "https://blob.handy.computer/moonshine-base.tar.gz",
        files: F_MOONSHINE_BASE,
        local_name: "moonshine-base",
        size_mb: 77,
        languages: &["en"],
        accuracy: 0.68,
        speed: 0.94,
        translate: false,
        live: true,
        recommended: false,
    },
];

pub fn find(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}
