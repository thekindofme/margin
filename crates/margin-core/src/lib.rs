//! Margin core: local-first meeting capture, transcription, speakers, calendar and AI notes.
pub mod agent;
pub mod audio;
pub mod audio_rec;
pub mod calendar;
pub mod catalog;
pub mod detect;
pub mod folders;
pub mod diarize;
pub mod ics;
pub mod llm;
pub mod mcp;
pub mod models;
pub mod names;
pub mod obsidian;
pub mod paths;
pub mod permissions;
pub mod session;
pub mod store;
pub mod stt;
pub mod types;
pub mod vad;

pub use types::*;
pub use transcribe_rs;
pub use crossbeam_channel::{bounded, Receiver, Sender};
