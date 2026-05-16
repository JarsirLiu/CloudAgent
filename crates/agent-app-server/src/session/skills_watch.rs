use crate::routing::command_router::ServerState;
use crate::session::service::notify_skills_changed;
use agent_core::AgentHost;
use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher, recommended_watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Duration, Instant};

const SKILL_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

pub(crate) fn spawn_skill_watch(
    runtime: Arc<AgentHost>,
    event_tx: mpsc::UnboundedSender<agent_protocol::AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
) {
    let roots = runtime.skill_watch_roots();
    if roots.is_empty() {
        return;
    }

    let watch_targets = dedupe_paths(roots.into_iter().map(|root| watch_target_for_root(&root)));
    if watch_targets.is_empty() {
        return;
    }

    let (change_tx, mut change_rx) = mpsc::unbounded_channel::<()>();
    let mut watcher = match build_watcher(change_tx) {
        Ok(watcher) => watcher,
        Err(err) => {
            tracing::warn!("failed to start skill watcher: {err:#}");
            return;
        }
    };

    for target in &watch_targets {
        if let Err(err) = watcher.watch(target, RecursiveMode::Recursive) {
            tracing::warn!("failed to watch skills path {}: {err:#}", target.display());
        }
    }

    tokio::spawn(async move {
        let _watcher = watcher;
        let mut pending = false;
        let mut deadline = Instant::now() + SKILL_WATCH_DEBOUNCE;
        loop {
            tokio::select! {
                maybe_change = change_rx.recv() => {
                    if maybe_change.is_none() {
                        break;
                    }
                    pending = true;
                    deadline = Instant::now() + SKILL_WATCH_DEBOUNCE;
                }
                _ = tokio::time::sleep_until(deadline), if pending => {
                    pending = false;
                    notify_skills_changed(&event_tx, &state).await;
                }
            }
        }
    });
}

fn build_watcher(change_tx: mpsc::UnboundedSender<()>) -> Result<RecommendedWatcher> {
    Ok(recommended_watcher(move |event: notify::Result<Event>| {
        if event.is_ok() {
            let _ = change_tx.send(());
        }
    })?)
}

fn watch_target_for_root(root: &Path) -> PathBuf {
    if root.exists() {
        return root.to_path_buf();
    }

    root.ancestors()
        .find(|candidate| candidate.exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf())
}

fn dedupe_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        if seen.insert(canonical.clone()) {
            deduped.push(canonical);
        }
    }
    deduped
}
