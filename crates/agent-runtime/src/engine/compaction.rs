use crate::{AgentRuntime, MANUAL_COMPACTION_MIN_HISTORY_TOKENS, ManualCompactionOutcome, tasks};
use anyhow::Result;

impl AgentRuntime {
    pub async fn compact_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ManualCompactionOutcome> {
        self.rollout_recorder.flush().await?;
        tasks::run_manual_compaction(self, conversation_id, MANUAL_COMPACTION_MIN_HISTORY_TOKENS)
            .await
    }
}
