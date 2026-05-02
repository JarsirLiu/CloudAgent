use super::common::{collect_repo_entries, rank_file_match, sort_ranked_paths};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolExecutionContext;
use agent_core::ToolSpec;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

pub struct FuzzyFileSearchTool;

#[derive(Debug, Clone, Deserialize)]
pub struct FuzzyFileSearchArgs {
    pub pattern: String,
    #[serde(default)]
    pub path_scope: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    #[serde(default)]
    pub offset: Option<usize>,
}

impl FuzzyFileSearchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "general"],
            ToolSpec {
                name: "fuzzy_file_search".to_string(),
                description: "Find the most likely files in a repository quickly. Use this before directory traversal when you need to locate a module, test, config, or implementation file by name, path fragment, or approximate match.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path_scope": { "type": "string" },
                        "max_results": { "type": "integer", "minimum": 1 },
                        "case_sensitive": { "type": "boolean" },
                        "offset": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["pattern"]
                }),
                mutating: false,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
    }
}

pub(crate) struct FuzzyFileSearchLocalTool;

#[async_trait]
impl LocalTool for FuzzyFileSearchLocalTool {
    fn spec(&self) -> ToolSpec {
        FuzzyFileSearchTool::descriptor().spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: FuzzyFileSearchArgs = serde_json::from_value(arguments)?;
        let pattern = args.pattern.trim().to_string();
        if pattern.is_empty() {
            bail!("`pattern` must not be empty");
        }
        let max_results = args.max_results.unwrap_or(200).clamp(1, 2_000);
        let offset = args.offset.unwrap_or(0);
        let case_sensitive = args.case_sensitive.unwrap_or(false);
        let root = resolve_workspace_path(&ctx.workspace_root, args.path_scope.as_deref())?;
        let workspace_root = ctx.workspace_root.clone();
        let pattern = if case_sensitive {
            pattern
        } else {
            pattern.to_ascii_lowercase()
        };
        let matches = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let entries = collect_repo_entries(&workspace_root, &root)?;
            let mut ranked = entries
                .into_iter()
                .filter_map(|entry| {
                    let candidate_name = if case_sensitive {
                        entry.file_name.clone()
                    } else {
                        entry.file_name.to_ascii_lowercase()
                    };
                    let candidate_path = if case_sensitive {
                        entry.relative_path.clone()
                    } else {
                        entry.relative_path.to_ascii_lowercase()
                    };
                    rank_file_match(&candidate_path, &candidate_name, &pattern)
                        .map(|score| (score, entry.relative_path))
                })
                .collect::<Vec<_>>();
            sort_ranked_paths(&mut ranked);
            Ok(ranked.into_iter().map(|(_, path)| path).collect())
        })
        .await??;
        let matches = matches
            .into_iter()
            .skip(offset)
            .take(max_results)
            .collect::<Vec<_>>();
        let content = if matches.is_empty() {
            "No files found".to_string()
        } else {
            matches.join("\n")
        };
        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::FindFiles {
                file_count: matches.len(),
            }),
        })
    }
}
