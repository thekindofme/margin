//! The sync folder on disk.
use crate::crypto::{self, Key, VaultId};
use crate::merge::Record;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Records from other devices, keyed by meeting id.
pub type RecordsByMeeting = BTreeMap<String, Vec<Record>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultFile {
    pub format: u32,
    pub vault_id: String,
    pub key_id: u32,
    pub kcv: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCard {
    pub name: String,
    pub kind: String,
    pub last_seen: String,
}

#[derive(Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub vault_id: VaultId,
    pub key_id: u32,
    pub key: Key,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read_vault_file(root: &Path) -> Result<Option<VaultFile>> {
    let p = root.join("vault.json");
    if !p.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&std::fs::read(&p)?).context("vault.json is unreadable")?))
}

impl Vault {
    /// Create a brand-new vault in an empty folder.
    pub fn create(root: &Path) -> Result<Self> {
        if read_vault_file(root)?.is_some() {
            bail!("{} already holds a Milkrice vault; pair with it instead", root.display());
        }
        let v = Self { root: root.into(), vault_id: crypto::random_bytes(), key_id: 1, key: crypto::random_bytes() };
        v.write_vault_file()?;
        Ok(v)
    }

    pub fn write_vault_file(&self) -> Result<()> {
        let f = VaultFile { format: 1, vault_id: crypto::b64(&self.vault_id), key_id: self.key_id, kcv: crypto::kcv(&self.key) };
        write_atomic(&self.root.join("vault.json"), &serde_json::to_vec_pretty(&f)?)
    }

    /// Open an existing vault with a key from the keychain, checking it really is this vault's key.
    pub fn open(root: &Path, key: Key) -> Result<Self> {
        let f = read_vault_file(root)?.context("no Milkrice vault in that folder")?;
        if crypto::kcv(&key) != f.kcv {
            bail!("this device's key doesn't match the vault (it was reset or re-keyed); pair again");
        }
        let vault_id: VaultId = crypto::unb64(&f.vault_id)?.try_into().ok().context("bad vault id")?;
        Ok(Self { root: root.into(), vault_id, key_id: f.key_id, key })
    }

    fn record_path(&self, meeting: &str, device: &str) -> PathBuf {
        self.root.join("m").join(meeting).join(format!("{device}.rec"))
    }

    pub fn write_record(&self, r: &Record) -> Result<()> {
        let sealed = crypto::seal(&self.key, &self.vault_id, self.key_id, &serde_json::to_vec(r)?)?;
        write_atomic(&self.record_path(&r.id, &r.device), &sealed)
    }

    /// Every other device's records, grouped by meeting. Unreadable files are reported, not fatal.
    pub fn read_records(&self, own_device: &str) -> Result<(RecordsByMeeting, Vec<String>)> {
        let mut out: BTreeMap<String, Vec<Record>> = BTreeMap::new();
        let mut problems = Vec::new();
        let dir = self.root.join("m");
        let Ok(meetings) = std::fs::read_dir(&dir) else { return Ok((out, problems)) };
        for m in meetings.flatten() {
            let Ok(files) = std::fs::read_dir(m.path()) else { continue };
            for f in files.flatten() {
                let p = f.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with(".rec") || name == format!("{own_device}.rec") {
                    continue;
                }
                let parsed = std::fs::read(&p)
                    .map_err(anyhow::Error::from)
                    .and_then(|b| crypto::open(&self.key, &self.vault_id, &b))
                    .and_then(|plain| Ok(serde_json::from_slice::<Record>(&plain)?));
                match parsed {
                    Ok(r) => out.entry(r.id.clone()).or_default().push(r),
                    Err(e) => problems.push(format!("{}: {e}", p.display())),
                }
            }
        }
        Ok((out, problems))
    }

    /// This device's own records (to recover meetings it knows but has lost locally).
    pub fn own_record_exists(&self, meeting: &str, device: &str) -> bool {
        self.record_path(meeting, device).exists()
    }

    pub fn write_device(&self, device: &str, card: &DeviceCard) -> Result<()> {
        let sealed = crypto::seal(&self.key, &self.vault_id, self.key_id, &serde_json::to_vec(card)?)?;
        write_atomic(&self.root.join("devices").join(format!("{device}.dev")), &sealed)
    }

    pub fn devices(&self) -> Vec<(String, DeviceCard)> {
        let Ok(rd) = std::fs::read_dir(self.root.join("devices")) else { return vec![] };
        rd.flatten()
            .filter_map(|e| {
                let p = e.path();
                let id = p.file_stem()?.to_str()?.to_string();
                let plain = crypto::open(&self.key, &self.vault_id, &std::fs::read(&p).ok()?).ok()?;
                Some((id, serde_json::from_slice(&plain).ok()?))
            })
            .collect()
    }

    pub fn pair_dir(&self) -> PathBuf {
        self.root.join("pair")
    }

    pub fn write_plain(&self, rel: &str, bytes: &[u8]) -> Result<()> {
        write_atomic(&self.root.join(rel), bytes)
    }
}
