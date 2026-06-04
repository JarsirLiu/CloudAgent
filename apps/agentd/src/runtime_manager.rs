use agent_app_server::AppRuntimeManager;
use agent_core::AgentHost;
use agent_protocol::{CommandExecutionContext, SessionBootstrapContext};
use anyhow::Result;
use cli::agent_host::build_agent_host;
use cli::app::cli_settings::load_cli_settings;
use config::AgentConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AgentdRuntimeManager {
    default_workspace_root: PathBuf,
    startup_data_dir_override: Option<PathBuf>,
    runtimes: Arc<Mutex<HashMap<RuntimeCacheKey, Arc<AgentHost>>>>,
}

impl AgentdRuntimeManager {
    pub fn new(
        default_workspace_root: PathBuf,
        startup_data_dir_override: Option<PathBuf>,
    ) -> Self {
        Self {
            default_workspace_root,
            startup_data_dir_override,
            runtimes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn runtime_for_context(
        &self,
        workspace_root: Option<&str>,
        data_root_dir: Option<&str>,
    ) -> Result<Arc<AgentHost>> {
        let key = self.resolve_cache_key(workspace_root, data_root_dir);

        if let Some(runtime) = self
            .runtimes
            .lock()
            .expect("runtime cache poisoned")
            .get(&key)
            .cloned()
        {
            return Ok(runtime);
        }

        let runtime = build_runtime(&key.workspace_root, key.data_root_dir.as_deref())?;
        let mut guard = self.runtimes.lock().expect("runtime cache poisoned");
        Ok(guard.entry(key).or_insert_with(|| runtime.clone()).clone())
    }

    fn resolve_cache_key(
        &self,
        workspace_root: Option<&str>,
        data_root_dir: Option<&str>,
    ) -> RuntimeCacheKey {
        let workspace_root = workspace_root
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_workspace_root.clone());
        let data_root_dir = data_root_dir
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| self.startup_data_dir_override.clone());
        RuntimeCacheKey::new(&workspace_root, data_root_dir.as_deref())
    }
}

impl AppRuntimeManager for AgentdRuntimeManager {
    fn initial_runtime(&self) -> Result<Arc<AgentHost>> {
        self.runtime_for_context(None, None)
    }

    fn runtime_for_session(
        &self,
        session_context: Option<&SessionBootstrapContext>,
    ) -> Result<Arc<AgentHost>> {
        let workspace_root = session_context.and_then(|ctx| ctx.workspace_root.as_deref());
        let data_root_dir = session_context.and_then(|ctx| ctx.data_root_dir.as_deref());
        self.runtime_for_context(workspace_root, data_root_dir)
    }

    fn runtime_for_command(
        &self,
        command_context: Option<&CommandExecutionContext>,
    ) -> Result<Arc<AgentHost>> {
        let workspace_root = command_context.and_then(|ctx| ctx.workspace_root.as_deref());
        let data_root_dir = command_context.and_then(|ctx| ctx.data_root_dir.as_deref());
        self.runtime_for_context(workspace_root, data_root_dir)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RuntimeCacheKey {
    workspace_root: PathBuf,
    data_root_dir: Option<PathBuf>,
}

impl RuntimeCacheKey {
    fn new(workspace_root: &Path, data_root_dir: Option<&Path>) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            data_root_dir: data_root_dir.map(Path::to_path_buf),
        }
    }
}

fn build_runtime(workspace_root: &Path, data_root_dir: Option<&Path>) -> Result<Arc<AgentHost>> {
    let mut config = AgentConfig::load(workspace_root.to_path_buf())?;
    apply_data_dir_override(&mut config, data_root_dir);
    if let Ok(Some(settings)) = load_cli_settings(&config.runtime.conversation_store_dir) {
        config.cli.pre_llm_filter_enabled = settings.pre_llm_filter_enabled;
        config.cli.permission_mode = settings.permission_mode;
    }
    build_agent_host(config)
}

fn apply_data_dir_override(config: &mut AgentConfig, data_root_dir: Option<&Path>) {
    let Some(value) = data_root_dir else {
        return;
    };
    config.runtime.data_root_dir = if value.is_absolute() {
        value.to_path_buf()
    } else {
        config.workspace_root.join(value)
    };
    config.runtime.conversation_store_dir = config.runtime.data_root_dir.join("conversations");
    config.runtime.memory.root_dir = config.runtime.data_root_dir.join("state").join("memory");
}

#[cfg(test)]
mod tests {
    use super::{AgentdRuntimeManager, RuntimeCacheKey};
    use agent_protocol::{CommandExecutionContext, SessionBootstrapContext};
    use std::path::PathBuf;

    #[test]
    fn resolve_cache_key_uses_default_workspace_when_missing() {
        let manager = AgentdRuntimeManager::new(PathBuf::from("D:/default"), None);

        let key = manager.resolve_cache_key(None, None);

        assert_eq!(
            key,
            RuntimeCacheKey {
                workspace_root: PathBuf::from("D:/default"),
                data_root_dir: None,
            }
        );
    }

    #[test]
    fn session_context_overrides_workspace_and_data_root() {
        let manager = AgentdRuntimeManager::new(
            PathBuf::from("D:/default"),
            Some(PathBuf::from("D:/startup-data")),
        );

        let key = manager.resolve_cache_key(
            Some("D:/workspace-a"),
            Some("D:/workspace-a/.cloudagent-data"),
        );

        assert_eq!(
            key,
            RuntimeCacheKey {
                workspace_root: PathBuf::from("D:/workspace-a"),
                data_root_dir: Some(PathBuf::from("D:/workspace-a/.cloudagent-data")),
            }
        );
    }

    #[test]
    fn command_context_uses_explicit_data_root_when_present() {
        let manager = AgentdRuntimeManager::new(PathBuf::from("D:/default"), None);
        let context = CommandExecutionContext {
            session_id: Some("session-1".to_string()),
            workspace_id: None,
            workspace_root: Some("D:/workspace-b".to_string()),
            cwd: Some("D:/workspace-b/subdir".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/shared-data".to_string()),
        };

        let runtime_key = manager.resolve_cache_key(
            context.workspace_root.as_deref(),
            context.data_root_dir.as_deref(),
        );

        assert_eq!(
            runtime_key,
            RuntimeCacheKey {
                workspace_root: PathBuf::from("D:/workspace-b"),
                data_root_dir: Some(PathBuf::from("D:/shared-data")),
            }
        );
    }

    #[test]
    fn startup_data_root_is_used_as_fallback() {
        let manager = AgentdRuntimeManager::new(
            PathBuf::from("D:/default"),
            Some(PathBuf::from("D:/startup-data")),
        );

        let key = manager.resolve_cache_key(Some("D:/workspace-c"), None);

        assert_eq!(
            key,
            RuntimeCacheKey {
                workspace_root: PathBuf::from("D:/workspace-c"),
                data_root_dir: Some(PathBuf::from("D:/startup-data")),
            }
        );
    }

    #[test]
    fn runtime_selection_prefers_command_context_over_session_defaults() {
        let manager = AgentdRuntimeManager::new(PathBuf::from("D:/default"), None);
        let session = SessionBootstrapContext {
            session_id: Some("session-1".to_string()),
            source_domain: Some("local:cli".to_string()),
            workspace_root: Some("D:/workspace-session".to_string()),
            cwd: Some("D:/workspace-session".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/session-data".to_string()),
        };
        let command = CommandExecutionContext {
            session_id: Some("session-1".to_string()),
            workspace_id: None,
            workspace_root: Some("D:/workspace-command".to_string()),
            cwd: Some("D:/workspace-command".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/command-data".to_string()),
        };

        let session_key = manager.resolve_cache_key(
            session.workspace_root.as_deref(),
            session.data_root_dir.as_deref(),
        );
        let command_key = manager.resolve_cache_key(
            command.workspace_root.as_deref(),
            command.data_root_dir.as_deref(),
        );

        assert_ne!(session_key, command_key);
        assert_eq!(
            command_key.workspace_root,
            PathBuf::from("D:/workspace-command")
        );
        assert_eq!(
            command_key.data_root_dir,
            Some(PathBuf::from("D:/command-data"))
        );
    }
}
