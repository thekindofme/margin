use std::path::PathBuf;

/// ~/Library/Application Support/Margin (or MARGIN_HOME for tests).
pub fn data_dir() -> PathBuf {
    let dir = std::env::var_os("MARGIN_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Margin"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn models_dir() -> PathBuf {
    let d = data_dir().join("models");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn audio_dir() -> PathBuf {
    let d = data_dir().join("audio");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn db_path() -> PathBuf {
    data_dir().join("margin.sqlite")
}
