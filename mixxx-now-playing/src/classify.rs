use std::path::Path;

pub fn is_v4v_track(track_path: &Path, v4v_root: &Path) -> bool {
    let Ok(track_path) = track_path.canonicalize() else {
        return false;
    };
    let root = v4v_root
        .canonicalize()
        .unwrap_or_else(|_| v4v_root.to_path_buf());
    track_path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn classify_accepts_tracks_under_canonical_v4v_root() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join("V4Vmusic");
        let nested = root.join("album");
        fs::create_dir_all(&nested)?;
        let track = nested.join("track.flac");
        fs::write(&track, b"fixture")?;

        assert!(is_v4v_track(&track, &root));
        Ok(())
    }

    #[test]
    fn classify_rejects_tracks_outside_root_or_missing_paths() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join("V4Vmusic");
        let other = temp.path().join("other");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&other)?;
        let track = other.join("track.flac");
        fs::write(&track, b"fixture")?;

        assert!(!is_v4v_track(&track, &root));
        assert!(!is_v4v_track(&root.join("missing.flac"), &root));
        Ok(())
    }
}
