#[cfg(test)]
use std::ffi::OsString;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
pub(crate) fn test_worker_program() -> OsString {
    let current_exe = std::env::current_exe().expect("current test executable");
    let deps_dir = current_exe.parent().expect("test executable parent");
    let target_dir = deps_dir.parent().expect("target profile directory");

    let candidates = [
        target_dir.join(exe_name("agentd")),
        deps_dir.join(exe_name("agentd")),
    ];

    candidates
        .iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            panic!(
                "failed to locate test worker binary; checked: {}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .clone()
        .into_os_string()
}

#[cfg(test)]
pub(crate) fn unique_temp_path(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    let counter = TEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{unique}-{counter}",
        std::process::id()
    ))
}

#[cfg(test)]
fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}
