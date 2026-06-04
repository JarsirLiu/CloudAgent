use std::ffi::OsStr;
use std::path::PathBuf;

pub(crate) fn resolve_data_root_dir(data_root_dir: Option<&OsStr>) -> PathBuf {
    data_root_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_data_root_dir)
}

fn default_data_root_dir() -> PathBuf {
    if release_mode_enabled() {
        return default_user_data_root().unwrap_or_else(|| PathBuf::from("data"));
    }

    std::env::current_dir()
        .map(|cwd| cwd.join(".cloudagent").join("data"))
        .unwrap_or_else(|_| PathBuf::from("data"))
}

fn release_mode_enabled() -> bool {
    std::env::var("CLOUDAGENT_RELEASE_MODE").ok().as_deref() == Some("1") || !cfg!(debug_assertions)
}

fn default_user_data_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|home| home.join(".cloudagent").join("data"))
}

#[cfg(test)]
mod tests {
    use super::resolve_data_root_dir;
    use std::ffi::OsStr;
    use std::path::PathBuf;

    #[test]
    fn explicit_data_root_wins() {
        let explicit = PathBuf::from(r"D:\repo\cloudagent\data");
        assert_eq!(
            resolve_data_root_dir(Some(OsStr::new(explicit.as_os_str()))),
            explicit
        );
    }

    #[test]
    fn dev_default_data_root_uses_workspace_cloudagent_directory_shape() {
        let explicit = PathBuf::from(r"D:\repo\cloudagent\.cloudagent\data");
        assert_eq!(
            resolve_data_root_dir(Some(OsStr::new(explicit.as_os_str()))),
            explicit
        );
    }
}
