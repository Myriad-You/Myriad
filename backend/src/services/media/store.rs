//! Durable media files under `DATA_DIR/media`. Temporary names are service-generated.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use super::error::MediaError;
use super::urls::parse_storage_key;

pub struct MediaStore {
    root: PathBuf,
}

impl MediaStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn temp_path(&self, write_token: Uuid) -> PathBuf {
        self.tmp_dir()
            .join(format!("{}.part", write_token.simple()))
    }

    pub fn final_path(&self, storage_key: &str) -> Result<PathBuf, MediaError> {
        parse_storage_key(storage_key)?;
        let mut path = self.root.clone();
        for part in storage_key.split('/') {
            path.push(part);
        }
        Ok(path)
    }

    pub async fn publish_bytes(
        &self,
        storage_key: &str,
        write_token: Uuid,
        bytes: &[u8],
    ) -> Result<(), MediaError> {
        self.stage_bytes(write_token, bytes).await?;
        self.publish_staged(storage_key, write_token).await
    }

    pub async fn stage_bytes(&self, write_token: Uuid, bytes: &[u8]) -> Result<(), MediaError> {
        tokio::fs::create_dir_all(self.tmp_dir()).await?;
        write_exclusive(&self.temp_path(write_token), bytes).await
    }

    /// Caller must hold the staging row lock and verify the current writer token.
    pub async fn publish_staged(
        &self,
        storage_key: &str,
        write_token: Uuid,
    ) -> Result<(), MediaError> {
        let final_path = self.final_path(storage_key)?;
        if let Some(parent) = final_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(self.temp_path(write_token), &final_path).await?;
        // Persist directory metadata before the database makes the file ready.
        sync_parent(&final_path).await?;
        Ok(())
    }

    pub async fn remove_owned_temp(&self, write_token: Uuid) -> Result<(), MediaError> {
        remove_if_exists(&self.temp_path(write_token)).await
    }

    pub async fn remove_final(&self, storage_key: &str) -> Result<(), MediaError> {
        let path = self.final_path(storage_key)?;
        remove_if_exists(&path).await
    }

    pub async fn final_checksum(&self, storage_key: &str) -> Result<Option<String>, MediaError> {
        let path = self.final_path(storage_key)?;
        match hash_file(&path).await {
            Ok((_, sum)) => Ok(Some(sum)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn temp_exists(&self, write_token: Uuid) -> bool {
        tokio::fs::metadata(self.temp_path(write_token))
            .await
            .is_ok()
    }

    /// Stream a legacy file into the persistent layout. Never unlinks the source.
    pub async fn publish_from_path(
        &self,
        storage_key: &str,
        write_token: Uuid,
        source: &Path,
    ) -> Result<CopyReport, MediaError> {
        let dest = self.final_path(storage_key)?;
        let (src_size, src_sum) = hash_path(source).await?;
        if tokio::fs::metadata(&dest).await.is_ok() {
            let (dest_size, dest_sum) = hash_path(&dest).await?;
            if dest_size == src_size && dest_sum == src_sum {
                sync_parent(&dest).await?;
                return Ok(CopyReport {
                    size: dest_size,
                    checksum_sha256: dest_sum,
                    wrote: false,
                });
            }
            return Err(MediaError::conflict(
                "Copied media does not match its source",
            ));
        }
        let tmp = self.temp_path(write_token);
        if let Some(parent) = tmp.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Err(error) = copy_exclusive(&tmp, source).await {
            let _ = remove_if_exists(&tmp).await;
            return Err(error);
        }
        let (tmp_size, tmp_sum) = match hash_path(&tmp).await {
            Ok(ok) => ok,
            Err(error) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(error);
            }
        };
        if tmp_size != src_size || tmp_sum != src_sum {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(MediaError::conflict(
                "Copied media does not match its source",
            ));
        }
        match tokio::fs::rename(&tmp, &dest).await {
            Ok(()) => {
                sync_parent(&dest).await?;
                Ok(CopyReport {
                    size: src_size,
                    checksum_sha256: src_sum,
                    wrote: true,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = tokio::fs::remove_file(&tmp).await;
                let (dest_size, dest_sum) = hash_path(&dest).await?;
                if dest_size == src_size && dest_sum == src_sum {
                    sync_parent(&dest).await?;
                    Ok(CopyReport {
                        size: dest_size,
                        checksum_sha256: dest_sum,
                        wrote: false,
                    })
                } else {
                    Err(MediaError::conflict(
                        "Copied media does not match its source",
                    ))
                }
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(error.into())
            }
        }
    }
}

#[cfg(unix)]
async fn sync_parent(path: &Path) -> Result<(), MediaError> {
    if let Some(parent) = path.parent() {
        tokio::fs::File::open(parent).await?.sync_all().await?;
    }
    Ok(())
}

/// Windows cannot open a directory as a file, so there is no handle to sync.
#[cfg(not(unix))]
async fn sync_parent(_path: &Path) -> Result<(), MediaError> {
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyReport {
    pub size: u64,
    pub checksum_sha256: String,
    pub wrote: bool,
}

async fn copy_exclusive(dest: &Path, source: &Path) -> Result<(), MediaError> {
    let mut input = match tokio::fs::File::open(source).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MediaError::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .await?;
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        output.write_all(&buf[..read]).await?;
    }
    output.flush().await?;
    output.sync_all().await?;
    Ok(())
}

pub(crate) async fn hash_path(path: &Path) -> Result<(u64, String), MediaError> {
    match hash_file(path).await {
        Ok(ok) => Ok(ok),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(MediaError::Missing),
        Err(error) => Err(error.into()),
    }
}

async fn write_exclusive(path: &Path, bytes: &[u8]) -> Result<(), MediaError> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    file.sync_all().await?;
    Ok(())
}

async fn remove_if_exists(path: &Path) -> Result<(), MediaError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buf[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::media::urls::storage_key;

    fn temp_store() -> (MediaStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("myriad-media-store-{}", Uuid::new_v4()));
        (MediaStore::new(root.clone()), root)
    }

    #[tokio::test]
    async fn publish_is_atomic_and_checksums() {
        let (store, root) = temp_store();
        let id = Uuid::new_v4();
        let key = storage_key(id, "png").unwrap();
        let token = Uuid::new_v4();
        let bytes = b"\x89PNG\r\n\x1a\nhello";
        store.publish_bytes(&key, token, bytes).await.unwrap();
        assert!(!store.temp_exists(token).await);
        let sum = store.final_checksum(&key).await.unwrap().unwrap();
        assert_eq!(
            sum,
            crate::services::media::validate::checksum_sha256(bytes)
        );
        store.remove_final(&key).await.unwrap();
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn cleanup_only_touches_this_writer_temp() {
        let (store, root) = temp_store();
        tokio::fs::create_dir_all(store.tmp_dir()).await.unwrap();
        let ours = Uuid::new_v4();
        let theirs = Uuid::new_v4();
        tokio::fs::write(store.temp_path(ours), b"a").await.unwrap();
        tokio::fs::write(store.temp_path(theirs), b"b")
            .await
            .unwrap();
        store.remove_owned_temp(ours).await.unwrap();
        assert!(!store.temp_exists(ours).await);
        assert!(store.temp_exists(theirs).await);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn rejects_storage_key_escape() {
        let store = MediaStore::new(PathBuf::from("/tmp/media"));
        assert!(store.final_path("../secret").is_err());
        assert!(store.final_path("/etc/passwd").is_err());
    }
}
