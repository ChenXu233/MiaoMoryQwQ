//! 本地磁盘 `StorageAdapter`（MVP 唯一实现，ADR-0008/白皮书 §3.5）。
//! key 形如 `ab/{sha256}.webp`，落到 `{thumbs_dir}/ab/{sha256}.webp`。

use std::path::PathBuf;

use mm_core::{ErrorCode, StorageAdapter};

pub struct LocalDiskAdapter {
    root: PathBuf,
}

impl LocalDiskAdapter {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn resolve(&self, key: &str) -> Result<PathBuf, ErrorCode> {
        // 防目录穿越：key 只允许字母数字与 / . _ -
        if key.is_empty()
            || key.starts_with('/')
            || key.contains("..")
            || key.contains('\\')
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
        {
            return Err(ErrorCode::WriteFailed);
        }
        Ok(self.root.join(key))
    }
}

impl StorageAdapter for LocalDiskAdapter {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), ErrorCode> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| ErrorCode::WriteFailed)?;
        }
        std::fs::write(path, bytes).map_err(|_| ErrorCode::WriteFailed)
    }

    fn get(&self, key: &str) -> Result<Vec<u8>, ErrorCode> {
        std::fs::read(self.resolve(key)?).map_err(|_| ErrorCode::ReadFailed)
    }

    fn delete(&self, key: &str) -> Result<(), ErrorCode> {
        match std::fs::remove_file(self.resolve(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(ErrorCode::WriteFailed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter() -> (tempfile::TempDir, LocalDiskAdapter) {
        let dir = tempfile::tempdir().unwrap();
        let a = LocalDiskAdapter::new(dir.path().to_path_buf());
        (dir, a)
    }

    #[test]
    fn put_get_delete_roundtrip() {
        let (_dir, a) = adapter();
        a.put("ab/cd.webp", b"hello").unwrap();
        assert_eq!(a.get("ab/cd.webp").unwrap(), b"hello");
        a.delete("ab/cd.webp").unwrap();
        a.delete("ab/cd.webp").unwrap(); // 幂等
        assert_eq!(a.get("ab/cd.webp").unwrap_err(), ErrorCode::ReadFailed);
    }

    #[test]
    fn rejects_path_traversal() {
        let (_dir, a) = adapter();
        assert!(a.put("../evil.webp", b"x").is_err());
        assert!(a.put("a\\b.webp", b"x").is_err());
        assert!(a.put("", b"x").is_err());
        assert!(a.put("/abs.webp", b"x").is_err());
    }
}
