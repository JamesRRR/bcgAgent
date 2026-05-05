use crate::error::{AppError, AppResult};
use crate::paths::secrets_dir;

fn read_key(filename: &str, label: &'static str) -> AppResult<String> {
    let path = secrets_dir().join(filename);
    let raw = std::fs::read_to_string(&path).map_err(|_| AppError::MissingKey(label))?;
    let key = raw.trim().to_string();
    if key.is_empty() {
        return Err(AppError::MissingKey(label));
    }
    Ok(key)
}

pub fn dashscope_key() -> AppResult<String> {
    read_key("dashscope.key", "dashscope")
}

pub fn minimax_key() -> AppResult<String> {
    read_key("minimax.key", "minimax")
}

/// Read a secret as `Option<String>`. Empty / missing file → `None`.
pub fn get_secret(name: &str) -> AppResult<Option<String>> {
    let path = secrets_dir().join(format!("{name}.key"));
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

/// Write a secret. Empty `value` deletes the file. Sets 0600 on unix.
pub fn set_secret(name: &str, value: &str) -> AppResult<()> {
    std::fs::create_dir_all(secrets_dir())?;
    let path = secrets_dir().join(format!("{name}.key"));
    if value.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    std::fs::write(&path, value.trim())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }
    Ok(())
}
