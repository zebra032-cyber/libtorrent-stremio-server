//! Chooses the folder torrent downloads are stored in.
//!
//! The settings app (and Stremio's `/settings` endpoint) saves a `cacheRoot`, but the engine used
//! to be created with the default per-user cache folder regardless, so a chosen folder never took
//! effect. This resolves the folder the engine should actually use at startup.

use std::path::{Path, PathBuf};

/// Where torrent downloads go.
///
/// * An explicit `cache_dir` in the server config always wins: that is how embedders (for example
///   the Android app) place the cache, so a saved setting must not override it.
/// * Otherwise a saved `cacheRoot` is used if it is an absolute path that can be created and
///   written to.
/// * Anything else (unset, relative, unwritable, missing drive) falls back to `default_dir`, with
///   a warning, so a bad setting can never stop the server from starting.
pub(crate) async fn resolve(
    explicit_cache_dir: bool,
    default_dir: &Path,
    cache_root: &str,
) -> PathBuf {
    if explicit_cache_dir {
        return default_dir.to_path_buf();
    }

    let configured = cache_root.trim();
    if configured.is_empty() || Path::new(configured) == default_dir {
        return default_dir.to_path_buf();
    }

    let candidate = PathBuf::from(configured);
    if !candidate.is_absolute() {
        tracing::warn!(
            path = %configured,
            fallback = ?default_dir,
            "Configured cache folder (cacheRoot) must be an absolute path; using the default"
        );
        return default_dir.to_path_buf();
    }

    match ensure_writable_dir(&candidate).await {
        Ok(()) => {
            tracing::info!(path = ?candidate, "Using cache/download folder from settings (cacheRoot)");
            candidate
        }
        Err(error) => {
            tracing::warn!(
                path = ?candidate,
                %error,
                fallback = ?default_dir,
                "Configured cache folder (cacheRoot) is not usable; using the default"
            );
            default_dir.to_path_buf()
        }
    }
}

async fn ensure_writable_dir(dir: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let probe = dir.join(".stream-server-write-test");
    tokio::fs::write(&probe, b"ok").await?;
    let _ = tokio::fs::remove_file(&probe).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn uses_saved_folder_when_writable() {
        let default = tempfile::tempdir().unwrap();
        let chosen = tempfile::tempdir().unwrap();
        let target = chosen.path().join("stremio-cache");

        let got = resolve(false, default.path(), target.to_str().unwrap()).await;

        assert_eq!(got, target);
        assert!(target.is_dir(), "the chosen folder is created");
        assert!(
            !target.join(".stream-server-write-test").exists(),
            "the write probe is cleaned up"
        );
    }

    #[tokio::test]
    async fn explicit_config_dir_wins_over_saved_folder() {
        let default = tempfile::tempdir().unwrap();
        let chosen = tempfile::tempdir().unwrap();

        let got = resolve(true, default.path(), chosen.path().to_str().unwrap()).await;

        assert_eq!(got, default.path());
    }

    #[tokio::test]
    async fn empty_or_relative_setting_falls_back_to_default() {
        let default = tempfile::tempdir().unwrap();

        assert_eq!(resolve(false, default.path(), "").await, default.path());
        assert_eq!(resolve(false, default.path(), "   ").await, default.path());
        assert_eq!(
            resolve(false, default.path(), "relative/cache").await,
            default.path()
        );
    }

    #[tokio::test]
    async fn unusable_folder_falls_back_to_default() {
        let default = tempfile::tempdir().unwrap();
        // A path *through a file* can never be created as a folder.
        let file = default.path().join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        let impossible = file.join("cache");

        let got = resolve(false, default.path(), impossible.to_str().unwrap()).await;

        assert_eq!(got, default.path());
    }
}
