use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn cache_home() -> Option<PathBuf> {
    if let Ok(xdg_cache_home) = std::env::var("XDG_CACHE_HOME") {
        if !xdg_cache_home.is_empty() {
            return Some(PathBuf::from(xdg_cache_home));
        }
    }

    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".cache"))
}

pub fn spotifm_cache_dir(playlist_path: Option<&Path>) -> PathBuf {
    cache_home()
        .map(|path| path.join("spotifm"))
        .unwrap_or_else(|| {
            playlist_path
                .and_then(Path::parent)
                .map(|parent| parent.join(".spotifm_cache"))
                .unwrap_or_else(|| PathBuf::from(".spotifm_cache"))
        })
}

pub fn find_librespot_credentials(cache_root: &Path) -> io::Result<Option<PathBuf>> {
    let direct = cache_root.join("librespot").join("credentials.json");
    if direct.is_file() {
        return Ok(Some(direct));
    }

    let entries = match fs::read_dir(cache_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };

    for entry in entries.flatten() {
        let candidate = entry.path().join("librespot").join("credentials.json");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

pub fn import_librespot_credentials(
    cache_root: &Path,
    spotifm_cache_dir: &Path,
) -> io::Result<Option<PathBuf>> {
    let destination = spotifm_cache_dir.join("credentials.json");
    if destination.is_file() {
        return Ok(None);
    }

    let Some(source) = find_librespot_credentials(cache_root)? else {
        return Ok(None);
    };

    fs::create_dir_all(spotifm_cache_dir)?;
    fs::copy(&source, &destination)?;
    Ok(Some(source))
}

#[cfg(test)]
mod tests {
    use super::{find_librespot_credentials, import_librespot_credentials};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(test_name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "spotifm-{test_name}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_direct_librespot_credentials_first() {
        let temp = TempDir::new("direct-credentials");
        let direct = temp.0.join("librespot/credentials.json");
        let nested = temp.0.join("ncspot/librespot/credentials.json");
        fs::create_dir_all(direct.parent().unwrap()).unwrap();
        fs::create_dir_all(nested.parent().unwrap()).unwrap();
        fs::write(&direct, "direct").unwrap();
        fs::write(&nested, "nested").unwrap();

        assert_eq!(find_librespot_credentials(&temp.0).unwrap(), Some(direct));
    }

    #[test]
    fn finds_and_imports_nested_librespot_credentials() {
        let temp = TempDir::new("nested-credentials");
        let source = temp.0.join("ncspot/librespot/credentials.json");
        let destination_dir = temp.0.join("spotifm");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "cached-credentials").unwrap();

        assert_eq!(
            import_librespot_credentials(&temp.0, &destination_dir).unwrap(),
            Some(source)
        );
        assert_eq!(
            fs::read_to_string(destination_dir.join("credentials.json")).unwrap(),
            "cached-credentials"
        );
    }

    #[test]
    fn existing_spotifm_credentials_are_not_overwritten() {
        let temp = TempDir::new("existing-credentials");
        let source = temp.0.join("librespot/credentials.json");
        let destination_dir = temp.0.join("spotifm");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(&destination_dir).unwrap();
        fs::write(source, "imported").unwrap();
        fs::write(destination_dir.join("credentials.json"), "existing").unwrap();

        assert_eq!(
            import_librespot_credentials(&temp.0, &destination_dir).unwrap(),
            None
        );
        assert_eq!(
            fs::read_to_string(destination_dir.join("credentials.json")).unwrap(),
            "existing"
        );
    }
}
