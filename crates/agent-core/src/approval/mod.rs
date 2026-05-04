mod runtime;

use crate::tool::ApprovalGrantKey;
use anyhow::Result;
use async_trait::async_trait;

pub(crate) use runtime::{ApprovalFlow, ApprovalRuntime};

#[async_trait]
pub trait ApprovalGrantStoreBackend: Send + Sync {
    async fn has_approval_grant(
        &self,
        conversation_id: &str,
        key: &ApprovalGrantKey,
    ) -> Result<bool>;

    async fn save_approval_grant(
        &self,
        conversation_id: &str,
        key: &ApprovalGrantKey,
    ) -> Result<()>;
}
