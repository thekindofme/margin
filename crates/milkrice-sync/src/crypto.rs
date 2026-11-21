//! Sealed files. Every file the sync folder holds (records, device cards) uses this layout:
//!
//! ```text
//! "MRS1" | vault_id (16) | key_id (u32 BE) | nonce (12) | ChaCha20-Poly1305(ciphertext + tag)
//! ```
//!
//! The first 24 bytes are bound as associated data, so a file can't be moved to another vault or
//! re-labelled with another key id. The plaintext is `u32 BE length | payload | zero padding` up to
//! a multiple of [`PAD`] bytes, which hides exact transcript lengths from the storage provider.
use anyhow::{anyhow, bail, ensure, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chacha20poly1305::{aead::Aead, aead::Payload, ChaCha20Poly1305, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

pub const MAGIC: &[u8; 4] = b"MRS1";
pub const HEADER_LEN: usize = 24;
pub const NONCE_LEN: usize = 12;
pub const PAD: usize = 1024;

pub type Key = [u8; 32];
pub type VaultId = [u8; 16];

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim()).map_err(|e| anyhow!("bad base64: {e}"))
}

fn header(vault: &VaultId, key_id: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..4].copy_from_slice(MAGIC);
    h[4..20].copy_from_slice(vault);
    h[20..24].copy_from_slice(&key_id.to_be_bytes());
    h
}

fn pad(payload: &[u8]) -> Vec<u8> {
    let used = 4 + payload.len();
    let total = used.div_ceil(PAD) * PAD;
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out.resize(total, 0);
    out
}

fn unpad(plain: &[u8]) -> Result<Vec<u8>> {
    ensure!(plain.len() >= 4, "sealed payload too short");
    let n = u32::from_be_bytes(plain[..4].try_into().unwrap()) as usize;
    ensure!(4 + n <= plain.len(), "sealed payload length is wrong");
    Ok(plain[4..4 + n].to_vec())
}

/// Seal with an explicit nonce (test vectors). Use [`seal`] everywhere else.
pub fn seal_with_nonce(key: &Key, vault: &VaultId, key_id: u32, nonce: &[u8; NONCE_LEN], payload: &[u8]) -> Result<Vec<u8>> {
    let h = header(vault, key_id);
    let cipher = ChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(Nonce::from_slice(nonce), Payload { msg: &pad(payload), aad: &h })
        .map_err(|_| anyhow!("encryption failed"))?;
    let mut out = Vec::with_capacity(HEADER_LEN + NONCE_LEN + ct.len());
    out.extend_from_slice(&h);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn seal(key: &Key, vault: &VaultId, key_id: u32, payload: &[u8]) -> Result<Vec<u8>> {
    seal_with_nonce(key, vault, key_id, &random_bytes::<NONCE_LEN>(), payload)
}

/// The key id a sealed file says it was written with (to pick the right key during rotation).
pub fn key_id_of(file: &[u8]) -> Option<u32> {
    (file.len() >= HEADER_LEN && &file[..4] == MAGIC).then(|| u32::from_be_bytes(file[20..24].try_into().unwrap()))
}

pub fn open(key: &Key, vault: &VaultId, file: &[u8]) -> Result<Vec<u8>> {
    ensure!(file.len() > HEADER_LEN + NONCE_LEN + 16, "file too short");
    if &file[..4] != MAGIC {
        bail!("not a Milkrice sync file");
    }
    ensure!(&file[4..20] == vault, "file belongs to another vault");
    let (h, rest) = file.split_at(HEADER_LEN);
    let (nonce, ct) = rest.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(key.into());
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: h })
        .map_err(|_| anyhow!("file failed authentication (wrong key or tampered)"))?;
    unpad(&plain)
}

/// Key check value: lets a device tell "wrong key" from "corrupt file" without decrypting records.
pub fn kcv(key: &Key) -> String {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).unwrap();
    m.update(b"milkrice-kcv-v1");
    hex::encode(&m.finalize().into_bytes()[..8])
}

/// A small AEAD box (nonce || ciphertext+tag) for pairing messages.
pub fn box_seal(key: &Key, aad: &[u8], nonce: &[u8; NONCE_LEN], msg: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(Nonce::from_slice(nonce), Payload { msg, aad })
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok([nonce.as_slice(), &ct].concat())
}

pub fn box_open(key: &Key, aad: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.len() > NONCE_LEN + 16, "box too short");
    let (nonce, ct) = data.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| anyhow!("pairing message failed authentication"))
}
