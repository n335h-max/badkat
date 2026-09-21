use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum LoadJson<T> {
    Missing,
    Primary(T),
    Backup(T),
    Invalid {
        primary: String,
        backup: Option<String>,
    },
}

pub fn load_json<T: DeserializeOwned>(path: &Path) -> LoadJson<T> {
    let primary = fs::read_to_string(path);
    match &primary {
        Ok(raw) => {
            if let Ok(value) = serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
                return LoadJson::Primary(value);
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }

    let backup_path = sibling(path, "bak");
    let backup = fs::read_to_string(&backup_path);
    if let Ok(raw) = &backup {
        if let Ok(value) = serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
            return LoadJson::Backup(value);
        }
    }

    let primary_missing = matches!(&primary, Err(err) if err.kind() == io::ErrorKind::NotFound);
    let backup_missing = matches!(&backup, Err(err) if err.kind() == io::ErrorKind::NotFound);
    if primary_missing && backup_missing {
        return LoadJson::Missing;
    }

    LoadJson::Invalid {
        primary: read_error(primary, "primary JSON is invalid"),
        backup: if backup_missing {
            None
        } else {
            Some(read_error(backup, "backup JSON is invalid"))
        },
    }
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let temp_path = sibling(path, "tmp");
    let backup_path = sibling(path, "bak");

    let result = (|| {
        let mut temp = File::create(&temp_path)?;
        temp.write_all(text.as_bytes())?;
        temp.sync_all()?;
        drop(temp);

        let primary_exists = path.exists();
        let primary_valid = fs::read_to_string(path)
            .ok()
            .and_then(|raw| {
                serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}')).ok()
            })
            .is_some();

        if primary_valid {
            if backup_path.exists() {
                fs::remove_file(&backup_path)?;
            }
            fs::rename(path, &backup_path)?;
        } else if primary_exists {
            // Never promote a corrupt primary into the recovery slot.
            // An existing valid backup remains untouched.
            fs::remove_file(path)?;
        }

        if let Err(err) = fs::rename(&temp_path, path) {
            if primary_valid && !path.exists() {
                let _ = fs::copy(&backup_path, path);
            }
            return Err(err);
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!("{ext}.{suffix}"))
        .unwrap_or_else(|| suffix.into());
    path.with_extension(extension)
}

fn read_error(result: io::Result<String>, invalid: &str) -> String {
    match result {
        Ok(_) => invalid.into(),
        Err(err) => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Value {
        name: String,
    }

    fn value(name: &str) -> Value {
        Value { name: name.into() }
    }

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "badkat-storage-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn successful_save_keeps_previous_value_as_backup() {
        let dir = temp_dir();
        let path = dir.join("value.json");
        save_json(&path, &value("A")).unwrap();
        save_json(&path, &value("B")).unwrap();

        assert!(matches!(load_json::<Value>(&path), LoadJson::Primary(v) if v == value("B")));
        assert!(matches!(
            load_json::<Value>(&path.with_extension("json.bak")),
            LoadJson::Primary(v) if v == value("A")
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_primary_recovers_from_valid_backup() {
        let dir = temp_dir();
        let path = dir.join("value.json");
        fs::write(&path, "not json").unwrap();
        fs::write(path.with_extension("json.bak"), r#"{"name":"A"}"#).unwrap();

        assert!(matches!(load_json::<Value>(&path), LoadJson::Backup(v) if v == value("A")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_files_are_not_reported_as_corruption() {
        let dir = temp_dir();
        let path = dir.join("value.json");
        assert!(matches!(load_json::<Value>(&path), LoadJson::Missing));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_primary_and_backup_are_reported() {
        let dir = temp_dir();
        let path = dir.join("value.json");
        fs::write(&path, "bad primary").unwrap();
        fs::write(path.with_extension("json.bak"), "bad backup").unwrap();

        assert!(matches!(
            load_json::<Value>(&path),
            LoadJson::Invalid { .. }
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacement_failure_keeps_the_previous_primary_readable() {
        let dir = temp_dir();
        let path = dir.join("value.json");
        save_json(&path, &value("A")).unwrap();
        // A directory at the backup path makes rotation fail after the
        // new temporary file has already been written.
        fs::create_dir(path.with_extension("json.bak")).unwrap();

        assert!(save_json(&path, &value("B")).is_err());
        assert!(matches!(load_json::<Value>(&path), LoadJson::Primary(v) if v == value("A")));
        fs::remove_dir_all(dir).unwrap();
    }
}
