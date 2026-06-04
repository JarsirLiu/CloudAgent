use agent_core::AgentHost;
use agent_protocol::{CommandExecutionContext, SessionBootstrapContext};
use anyhow::Result;
use std::sync::Arc;

pub trait AppRuntimeManager: Send + Sync {
    fn initial_runtime(&self) -> Result<Arc<AgentHost>>;

    fn runtime_for_session(
        &self,
        session_context: Option<&SessionBootstrapContext>,
    ) -> Result<Arc<AgentHost>> {
        let _ = session_context;
        self.initial_runtime()
    }

    fn runtime_for_command(
        &self,
        command_context: Option<&CommandExecutionContext>,
    ) -> Result<Arc<AgentHost>> {
        let _ = command_context;
        self.initial_runtime()
    }
}

pub struct FixedRuntimeManager {
    runtime: Arc<AgentHost>,
}

impl FixedRuntimeManager {
    pub fn new(runtime: Arc<AgentHost>) -> Self {
        Self { runtime }
    }
}

impl AppRuntimeManager for FixedRuntimeManager {
    fn initial_runtime(&self) -> Result<Arc<AgentHost>> {
        Ok(self.runtime.clone())
    }
}
