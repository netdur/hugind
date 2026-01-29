use std::path::PathBuf;

/// Returns the configuration home directory.
/// 
/// - macOS/Linux: $XDG_CONFIG_HOME/hugind or ~/.hugind
/// - Windows: %APPDATA%\hugind or %USERPROFILE%\.hugind
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
    
    // Fallback to ~/.hugind
    if let Some(home) = dirs::home_dir() {
        return home.join(".hugind");
    }

    PathBuf::from(".hugind")
}

/// Returns the data home directory.
/// 
/// - macOS/Linux: ~/.hugind
/// - Windows: %USERPROFILE%\.hugind
pub fn data_home() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".hugind");
    }
    PathBuf::from(".hugind")
}

/// Returns the directory where config files are stored.
pub fn configs_dir() -> PathBuf {
    config_home().join("configs")
}

/// Returns the directory where agents are stored.
pub fn agents_dir() -> PathBuf {
    config_home().join("agents")
}

/// Returns the directory where sessions are stored.
pub fn sessions_dir() -> PathBuf {
    data_home().join("sessions")
}
