// Speech to text: Parakeet TDT 0.6B v3 (ONNX, int8) through transcribe-rs.
// 6.3% English WER, 25 languages, faster than realtime on a plain CPU.
//
// Models are NOT bundled. First run downloads the one in `builtin()`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

pub trait Engine: Send {
    fn transcribe(&mut self, samples_16k: &[f32]) -> anyhow::Result<String>;
}

pub const PARAKEET_DIR: &str = "parakeet-tdt-0.6b-v3-int8";

/// Where the ONNX export lives and what it consists of. Sizes are
/// approximate and only drive the progress bar.
pub const PARAKEET_URL: &str = "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main";
pub const PARAKEET_FILES: &[&str] = &[
    "config.json",
    "vocab.txt",
    "nemo128.onnx",
    "decoder_joint-model.int8.onnx",
    "encoder-model.int8.onnx",
];
pub const PARAKEET_SIZES: &[u64] = &[97, 93_939, 139_764, 18_202_004, 652_183_999];

/// Everything needed to fetch and find one model. The built-in one ships in
/// the binary; a newer one can arrive through the manifest below and is then
/// kept in settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// Folder name under the models dir.
    pub id: String,
    /// Shown in Settings.
    pub name: String,
    pub url: String,
    pub files: Vec<String>,
    pub sizes: Vec<u64>,
}

pub fn builtin() -> ModelSpec {
    ModelSpec {
        id: PARAKEET_DIR.into(),
        name: "Parakeet TDT 0.6B v3".into(),
        url: PARAKEET_URL.into(),
        files: PARAKEET_FILES.iter().map(|s| s.to_string()).collect(),
        sizes: PARAKEET_SIZES.to_vec(),
    }
}

impl ModelSpec {
    pub fn total(&self) -> u64 {
        self.sizes.iter().sum()
    }
}

/// A small JSON file, `{"model": ModelSpec}`, that names the model neptune
/// should be using. Checked from Settings at most once a day.
pub const MODEL_MANIFEST_URL: &str = "https://raw.githubusercontent.com/frishberg/Neptune/main/model.json";

/// Where model folders live. BLURT_MODELS overrides for development;
/// otherwise the per-user app data folder.
pub fn models_dir(app_data: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("BLURT_MODELS") {
        return PathBuf::from(dir);
    }
    app_data.join("models")
}

// ---------- Parakeet ----------

pub struct Parakeet {
    model: ParakeetModel,
}

impl Parakeet {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let model = ParakeetModel::load(&dir.to_path_buf(), &Quantization::Int8)?;
        Ok(Self { model })
    }
}

impl Engine for Parakeet {
    fn transcribe(&mut self, samples_16k: &[f32]) -> anyhow::Result<String> {
        let result = self
            .model
            .transcribe_with(samples_16k, &ParakeetParams::default())?;
        Ok(result.text.trim().to_string())
    }
}
