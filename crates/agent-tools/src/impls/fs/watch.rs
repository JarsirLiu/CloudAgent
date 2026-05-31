use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolLayer, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{
    StructuredToolResult, ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec,
    TurnItemDeltaKind, TurnItemKind,
};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher, recommended_watcher};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

pub struct WatchTool;

impl WatchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["verify", "fs"],
            ToolUsageGuidance {
                selection_priority: -1,
                preferred_for: vec![
                    "watching one known file or directory for later changes",
                    "tracking filesystem churn outside the normal code-reading path",
                ],
                avoid_for: vec![
                    "normal repository reading",
                    "one-off metadata checks",
                ],
                follow_up_hint: Some(
                    "use `unwatch` with the same `watch_id` to stop the watch and retrieve the changed paths collected so far",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "watch".to_string(),
                identity: ToolIdentity::built_in("watch"),
                description:
                    "Watch one known file or directory path for future filesystem changes. This is a deferred filesystem primitive, not a repository reading tool."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "watch_id": { "type": "string" },
                        "path": { "type": "string" },
                        "recursive": { "type": "boolean" }
                    },
                    "required": ["watch_id", "path"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::PlatformFs)
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

pub struct UnwatchTool;

impl UnwatchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["verify", "fs"],
            ToolUsageGuidance {
                selection_priority: -1,
                preferred_for: vec![
                    "stopping a prior filesystem watch",
                    "collecting the changed paths observed since `watch` was started",
                ],
                avoid_for: vec![
                    "normal repository reading",
                    "filesystem writes",
                ],
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "unwatch".to_string(),
                identity: ToolIdentity::built_in("unwatch"),
                description:
                    "Stop a prior `watch` registration and return the changed paths collected for that watch."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "watch_id": { "type": "string" }
                    },
                    "required": ["watch_id"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::PlatformFs)
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Clone)]
pub(crate) struct WatchManager {
    state: Arc<Mutex<WatchState>>,
}

impl WatchManager {
    pub(crate) fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<Vec<PathBuf>>();
        let state = Arc::new(Mutex::new(WatchState::new(event_tx)));
        let state_for_thread = Arc::clone(&state);
        std::thread::Builder::new()
            .name("agent-tools-watch-manager".to_string())
            .spawn(move || {
                while let Ok(paths) = event_rx.recv() {
                    if let Ok(mut guard) = state_for_thread.lock() {
                        guard.record_changed_paths(paths);
                    } else {
                        break;
                    }
                }
            })
            .expect("watch manager thread should start");
        Self { state }
    }

    pub(crate) fn watch(&self, watch_id: &str, path: &Path, recursive: bool) -> Result<PathBuf> {
        let canonical_path = canonical_watch_target(path)?;
        let mut guard = self
            .state
            .lock()
            .map_err(|_| anyhow!("watch manager state is poisoned"))?;
        guard.register_watch(watch_id, canonical_path.clone(), recursive)?;
        Ok(canonical_path)
    }

    pub(crate) fn unwatch(&self, watch_id: &str) -> Result<UnwatchResult> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| anyhow!("watch manager state is poisoned"))?;
        guard.remove_watch(watch_id)
    }

    #[cfg(test)]
    fn record_changed_paths_for_test(&self, paths: Vec<PathBuf>) {
        let mut guard = self.state.lock().expect("watch manager lock");
        guard.record_changed_paths(paths);
    }
}

struct WatchState {
    watcher: Option<RecommendedWatcher>,
    event_tx: mpsc::Sender<Vec<PathBuf>>,
    registrations: HashMap<String, WatchRegistration>,
    watched_paths: HashMap<PathBuf, WatchRefCounts>,
}

impl WatchState {
    fn new(event_tx: mpsc::Sender<Vec<PathBuf>>) -> Self {
        Self {
            watcher: None,
            event_tx,
            registrations: HashMap::new(),
            watched_paths: HashMap::new(),
        }
    }

    fn register_watch(
        &mut self,
        watch_id: &str,
        canonical_path: PathBuf,
        recursive: bool,
    ) -> Result<()> {
        if self.registrations.contains_key(watch_id) {
            bail!("watch_id `{watch_id}` is already active");
        }
        self.ensure_watcher()?;
        self.add_watch_ref(&canonical_path, recursive)?;
        self.registrations.insert(
            watch_id.to_string(),
            WatchRegistration {
                canonical_path,
                recursive,
                changed_paths: BTreeSet::new(),
            },
        );
        Ok(())
    }

    fn remove_watch(&mut self, watch_id: &str) -> Result<UnwatchResult> {
        let Some(registration) = self.registrations.remove(watch_id) else {
            bail!("watch_id `{watch_id}` is not active");
        };
        self.remove_watch_ref(&registration.canonical_path, registration.recursive)?;
        let changed_paths = registration.changed_paths.into_iter().collect::<Vec<_>>();
        Ok(UnwatchResult {
            watch_id: watch_id.to_string(),
            changed_paths,
        })
    }

    fn ensure_watcher(&mut self) -> Result<()> {
        if self.watcher.is_some() {
            return Ok(());
        }
        let event_tx = self.event_tx.clone();
        let watcher = recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else {
                return;
            };
            let changed_paths = event.paths;
            if changed_paths.is_empty() {
                return;
            }
            let _ = event_tx.send(changed_paths);
        })?;
        self.watcher = Some(watcher);
        Ok(())
    }

    fn add_watch_ref(&mut self, path: &Path, recursive: bool) -> Result<()> {
        let next_mode = {
            let entry = self.watched_paths.entry(path.to_path_buf()).or_default();
            let previous_mode = entry.mode();
            if recursive {
                entry.recursive += 1;
            } else {
                entry.non_recursive += 1;
            }
            (previous_mode, entry.mode())
        };
        self.apply_watch_mode(path, next_mode.0, next_mode.1)
    }

    fn remove_watch_ref(&mut self, path: &Path, recursive: bool) -> Result<()> {
        let Some(entry) = self.watched_paths.get_mut(path) else {
            bail!("watch path bookkeeping is missing for `{}`", path.display());
        };
        let previous_mode = entry.mode();
        if recursive {
            entry.recursive = entry.recursive.saturating_sub(1);
        } else {
            entry.non_recursive = entry.non_recursive.saturating_sub(1);
        }
        let next_mode = entry.mode();
        let remove_path = entry.recursive == 0 && entry.non_recursive == 0;
        self.apply_watch_mode(path, previous_mode, next_mode)?;
        if remove_path {
            self.watched_paths.remove(path);
        }
        Ok(())
    }

    fn apply_watch_mode(
        &mut self,
        path: &Path,
        previous_mode: Option<RecursiveMode>,
        next_mode: Option<RecursiveMode>,
    ) -> Result<()> {
        let Some(watcher) = self.watcher.as_mut() else {
            bail!("filesystem watcher is not initialized");
        };
        match (previous_mode, next_mode) {
            (None, Some(mode)) => watcher.watch(path, mode)?,
            (Some(_), None) => watcher.unwatch(path)?,
            (Some(previous), Some(next)) if previous != next => {
                watcher.unwatch(path)?;
                watcher.watch(path, next)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn record_changed_paths(&mut self, paths: Vec<PathBuf>) {
        for changed_path in paths {
            let normalized_path = normalize_changed_path(&changed_path);
            for registration in self.registrations.values_mut() {
                if registration.matches(&normalized_path) {
                    registration
                        .changed_paths
                        .insert(normalized_path.display().to_string());
                }
            }
        }
    }
}

#[derive(Default)]
struct WatchRefCounts {
    recursive: usize,
    non_recursive: usize,
}

impl WatchRefCounts {
    fn mode(&self) -> Option<RecursiveMode> {
        if self.recursive > 0 {
            Some(RecursiveMode::Recursive)
        } else if self.non_recursive > 0 {
            Some(RecursiveMode::NonRecursive)
        } else {
            None
        }
    }
}

struct WatchRegistration {
    canonical_path: PathBuf,
    recursive: bool,
    changed_paths: BTreeSet<String>,
}

impl WatchRegistration {
    fn matches(&self, changed_path: &Path) -> bool {
        if self.recursive {
            changed_path == self.canonical_path || changed_path.starts_with(&self.canonical_path)
        } else if changed_path == self.canonical_path {
            true
        } else {
            changed_path.parent() == Some(self.canonical_path.as_path())
        }
    }
}

pub(crate) struct UnwatchResult {
    pub(crate) watch_id: String,
    pub(crate) changed_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WatchArgs {
    watch_id: String,
    path: String,
    #[serde(default)]
    recursive: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UnwatchArgs {
    watch_id: String,
}

pub(crate) struct WatchLocalTool {
    pub(crate) manager: WatchManager,
}

pub(crate) struct UnwatchLocalTool {
    pub(crate) manager: WatchManager,
}

#[async_trait]
impl LocalTool for WatchLocalTool {
    fn spec(&self) -> ToolSpec {
        WatchTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: WatchArgs = invocation.payload.parse_arguments()?;
        let recursive = args.recursive.unwrap_or(true);
        let path = resolve_read_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.path.as_str()),
        )?;
        let canonical_path = self.manager.watch(&args.watch_id, &path, recursive)?;
        Ok(ToolInvocationOutput {
            content: format!(
                "Watching `{}` with watch_id `{}`{}.",
                canonical_path.display(),
                args.watch_id,
                if recursive { " recursively" } else { "" }
            ),
            structured: Some(StructuredToolResult::Watch {
                watch_id: args.watch_id,
                path: canonical_path.display().to_string(),
                recursive,
                active: true,
            }),
        })
    }
}

#[async_trait]
impl LocalTool for UnwatchLocalTool {
    fn spec(&self) -> ToolSpec {
        UnwatchTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: UnwatchArgs = invocation.payload.parse_arguments()?;
        let result = self.manager.unwatch(&args.watch_id)?;
        let content = if result.changed_paths.is_empty() {
            format!(
                "Stopped watch `{}` with no observed path changes.",
                result.watch_id
            )
        } else {
            format!(
                "Stopped watch `{}` after observing {} changed path(s):\n{}",
                result.watch_id,
                result.changed_paths.len(),
                result.changed_paths.join("\n")
            )
        };
        Ok(ToolInvocationOutput {
            content,
            structured: Some(StructuredToolResult::Unwatch {
                watch_id: result.watch_id,
                removed: true,
                changed_path_count: result.changed_paths.len(),
                changed_paths: result.changed_paths,
            }),
        })
    }
}

fn canonical_watch_target(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("watch target `{}` does not exist", path.display());
    }
    Ok(path.canonicalize()?)
}

fn normalize_changed_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{LocalToolPayload, LocalToolSource};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[test]
    fn watch_manager_collects_changed_paths_until_unwatch() {
        let manager = WatchManager::new();
        let root = test_workspace("watch_manager_collects_changed_paths");
        let target = root.join("src");
        std::fs::create_dir_all(&target).expect("mkdir");
        let changed_file = target.join("lib.rs");
        std::fs::write(&changed_file, "fn demo() {}\n").expect("seed file");
        manager.watch("watch-1", &target, true).expect("watch");

        manager.record_changed_paths_for_test(vec![changed_file.clone()]);

        let result = manager.unwatch("watch-1").expect("unwatch");
        assert_eq!(
            result.changed_paths,
            vec![normalize_changed_path(&changed_file).display().to_string()]
        );
    }

    #[tokio::test]
    async fn watch_and_unwatch_tools_roundtrip_structured_results() {
        let manager = WatchManager::new();
        let workspace = test_workspace("watch_tools_roundtrip");
        let target = workspace.join("src");
        std::fs::create_dir_all(&target).expect("mkdir");
        let changed_file = target.join("main.rs");
        std::fs::write(&changed_file, "fn main() {}\n").expect("seed file");

        let watch_tool = WatchLocalTool {
            manager: manager.clone(),
        };
        watch_tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("watch"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "watch_id": "watch-1",
                            "path": target.display().to_string(),
                            "recursive": true
                        }),
                    },
                },
                &tool_context(&workspace),
            )
            .await
            .expect("watch works");

        manager.record_changed_paths_for_test(vec![changed_file]);

        let unwatch_tool = UnwatchLocalTool { manager };
        let output = unwatch_tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("unwatch"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "watch_id": "watch-1"
                        }),
                    },
                },
                &tool_context(&workspace),
            )
            .await
            .expect("unwatch works");

        assert!(matches!(
            output.structured,
            Some(StructuredToolResult::Unwatch {
                changed_path_count: 1,
                ..
            })
        ));
    }

    fn tool_context(workspace_root: &Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::ReadOnly,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn test_workspace(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        path.push(format!("cloudagent_{name}_{stamp}"));
        std::fs::create_dir_all(&path).expect("create temp workspace");
        path
    }
}
