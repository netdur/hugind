use std::path::PathBuf;

pub fn config_home() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join("hugind");
        }
    } else {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("hugind");
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".hugind");
    }

    PathBuf::from(".hugind")
}

pub fn data_home() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".hugind");
    }
    PathBuf::from(".hugind")
}

pub fn configs_dir() -> PathBuf {
    config_home().join("configs")
}

pub fn agents_dir() -> PathBuf {
    config_home().join("agents")
}

pub fn sessions_dir() -> PathBuf {
    data_home().join("sessions")
}

pub fn logs_dir() -> PathBuf {
    data_home().join("logs")
}

pub fn models_dir() -> PathBuf {
    data_home().join("models")
}
