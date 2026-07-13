use anyhow::Context;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static OUTPUT_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

fn read_array<const N: usize>(bytes: &[u8], offset: usize, kind: &str) -> anyhow::Result<[u8; N]> {
    bytes
        .get(offset..)
        .and_then(|tail| tail.get(..N))
        .with_context(|| format!("{kind} at byte offset {offset} is out of bounds"))?
        .try_into()
        .with_context(|| format!("{kind} at byte offset {offset} has invalid length"))
}

pub(crate) fn read_u16(bytes: &[u8], offset: usize) -> anyhow::Result<u16> {
    Ok(u16::from_le_bytes(read_array(bytes, offset, "u16")?))
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(read_array(bytes, offset, "u32")?))
}

pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(read_array(bytes, offset, "u64")?))
}

pub(crate) fn hex_u32(value: u32) -> String {
    format!("0x{value:08x}")
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &bytes)
}

pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    write_atomic(path, bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    create_parent(path)?;
    let temporary = temporary_sibling(path, "tmp");
    fs::write(&temporary, bytes)
        .with_context(|| format!("writing temporary output {}", temporary.display()))?;
    if let Err(error) = replace_path(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn create_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    Ok(())
}

fn temporary_sibling(path: &Path, kind: &str) -> PathBuf {
    let id = OUTPUT_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{name}.{kind}-{}-{id}", std::process::id()))
}

fn replace_path(staged: &Path, destination: &Path) -> anyhow::Result<()> {
    if !destination.exists() {
        return fs::rename(staged, destination).with_context(|| {
            format!(
                "committing staged output {} to {}",
                staged.display(),
                destination.display()
            )
        });
    }

    let backup = temporary_sibling(destination, "backup");
    fs::rename(destination, &backup).with_context(|| {
        format!(
            "moving previous output {} to {}",
            destination.display(),
            backup.display()
        )
    })?;
    if let Err(error) = fs::rename(staged, destination) {
        let restore_result = fs::rename(&backup, destination);
        return match restore_result {
            Ok(()) => Err(error).with_context(|| {
                format!(
                    "committing staged output {} to {}",
                    staged.display(),
                    destination.display()
                )
            }),
            Err(restore_error) => anyhow::bail!(
                "committing {} failed ({error}); restoring {} also failed ({restore_error})",
                destination.display(),
                backup.display()
            ),
        };
    }
    remove_path(&backup).with_context(|| format!("removing previous output {}", backup.display()))
}

fn remove_path(path: &Path) -> anyhow::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) struct StagedDirectory {
    destination: PathBuf,
    staging: PathBuf,
    committed: bool,
}

impl StagedDirectory {
    pub(crate) fn new(destination: &Path) -> anyhow::Result<Self> {
        create_parent(destination)?;
        let staging = temporary_sibling(destination, "staging");
        fs::create_dir(&staging)
            .with_context(|| format!("creating staging directory {}", staging.display()))?;
        Ok(Self {
            destination: destination.to_path_buf(),
            staging,
            committed: false,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.staging
    }

    pub(crate) fn commit(mut self) -> anyhow::Result<()> {
        replace_path(&self.staging, &self.destination)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedDirectory {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_root(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tyria-output-{name}-{}-{id}", std::process::id()))
    }

    #[test]
    fn staged_directory_replaces_only_on_commit() -> anyhow::Result<()> {
        let root = test_root("stage");
        let destination = root.join("skills");
        fs::create_dir_all(&destination)?;
        fs::write(destination.join("old.txt"), b"old")?;

        {
            let staging = StagedDirectory::new(&destination)?;
            fs::write(staging.path().join("new.txt"), b"new")?;
        }
        assert_eq!(fs::read(destination.join("old.txt"))?, b"old");

        let staging = StagedDirectory::new(&destination)?;
        fs::write(staging.path().join("new.txt"), b"new")?;
        staging.commit()?;
        assert!(!destination.join("old.txt").exists());
        assert_eq!(fs::read(destination.join("new.txt"))?, b"new");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn atomic_json_replaces_file_without_temporary_artifacts() -> anyhow::Result<()> {
        let root = test_root("json");
        let output = root.join("catalog.json");
        write_json(&output, &serde_json::json!({"value": 1}))?;
        write_json(&output, &serde_json::json!({"value": 2}))?;

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&output)?)?;
        assert_eq!(value["value"], 2);
        assert_eq!(fs::read_dir(&root)?.count(), 1);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
