use agent_protocol::TransportClientInfo;
use std::path::Path;

const PLATFORM_RUNTIME_CLIENT_PREFIX: &str = "node-platform-";
const PLATFORM_RUNTIME_FALLBACK_PREFIX: &str = "cloudagent-platform-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodeSource {
    domain_id: String,
    worker_scope_policy: WorkerScopePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerScopePolicy {
    // Local surfaces share the same resident node, but their worker identity is
    // derived from the target workspace instead of the node startup directory.
    WorkspaceScopedLocal,
    // Remote / IM style sources stay domain-scoped for now so one shared worker
    // can multiplex multiple sessions while still receiving explicit execution context.
    DomainScopedShared,
}

impl NodeSource {
    pub(crate) fn placeholder(session_scope_key: impl Into<String>) -> Self {
        let session_scope_key = sanitize_source_segment(&session_scope_key.into());
        Self {
            domain_id: format!("remote:{session_scope_key}"),
            worker_scope_policy: WorkerScopePolicy::DomainScopedShared,
        }
    }

    pub(crate) fn from_client_info(client_info: &TransportClientInfo) -> Self {
        let domain_id = source_domain_id(&client_info.name);
        Self::from_domain_id(domain_id)
    }

    pub(crate) fn from_domain_id(domain_id: impl Into<String>) -> Self {
        let domain_id = domain_id.into();
        Self {
            worker_scope_policy: worker_scope_policy_for_domain(&domain_id),
            domain_id,
        }
    }

    pub(crate) fn domain_id(&self) -> &str {
        &self.domain_id
    }

    #[cfg(test)]
    pub(crate) fn worker_scope_policy(&self) -> WorkerScopePolicy {
        self.worker_scope_policy
    }

    pub(crate) fn worker_scope_key(&self, workspace_root: Option<&Path>) -> String {
        match self.worker_scope_policy {
            WorkerScopePolicy::WorkspaceScopedLocal => {
                if let Some(workspace_root) = workspace_root {
                    // The worker key must remain stable for the same workspace
                    // even when multiple local CLI sessions connect over time.
                    let normalized = workspace_root
                        .to_string_lossy()
                        .replace('/', "\\")
                        .to_ascii_lowercase();
                    let hash = normalized
                        .bytes()
                        .fold(1469598103934665603u64, |acc, byte| {
                            acc.wrapping_mul(1099511628211)
                                .wrapping_add(u64::from(byte))
                        });
                    format!("{}@{hash:016x}", self.domain_id)
                } else {
                    self.domain_id.clone()
                }
            }
            WorkerScopePolicy::DomainScopedShared => self.domain_id.clone(),
        }
    }
}

pub(crate) fn platform_runtime_client_name(platform_id: &str) -> String {
    format!(
        "{PLATFORM_RUNTIME_CLIENT_PREFIX}{}",
        sanitize_source_segment(platform_id)
    )
}

fn source_domain_id(client_name: &str) -> String {
    let normalized = sanitize_source_segment(client_name);
    if let Some(platform_id) = parse_platform_runtime_client(&normalized) {
        return format!("im:{platform_id}");
    }

    match normalized.as_str() {
        "cloudagent" | "cli" | "cloudagent-cli" => "local:cli".to_string(),
        "web" | "cloudagent-web" => "local:web".to_string(),
        _ => {
            if let Some(local_kind) = parse_local_client_kind(&normalized) {
                format!("local:{local_kind}")
            } else if let Some(im_kind) = parse_known_im_kind(&normalized) {
                format!("im:{im_kind}")
            } else {
                format!("remote:{normalized}")
            }
        }
    }
}

fn worker_scope_policy_for_domain(domain_id: &str) -> WorkerScopePolicy {
    if domain_id.starts_with("local:") {
        WorkerScopePolicy::WorkspaceScopedLocal
    } else {
        WorkerScopePolicy::DomainScopedShared
    }
}

fn parse_platform_runtime_client(client_name: &str) -> Option<String> {
    client_name
        .strip_prefix(PLATFORM_RUNTIME_CLIENT_PREFIX)
        .or_else(|| client_name.strip_prefix(PLATFORM_RUNTIME_FALLBACK_PREFIX))
        .map(sanitize_source_segment)
        .filter(|platform_id| !platform_id.is_empty())
}

fn parse_local_client_kind(client_name: &str) -> Option<&'static str> {
    if client_name.contains("cli") {
        Some("cli")
    } else if client_name.contains("web") {
        Some("web")
    } else {
        None
    }
}

fn parse_known_im_kind(client_name: &str) -> Option<&'static str> {
    if client_name.contains("feishu") {
        Some("feishu")
    } else if client_name.contains("wecom") {
        Some("wecom")
    } else if client_name.contains("telegram") {
        Some("telegram")
    } else if client_name.contains("discord") {
        Some("discord")
    } else if client_name.contains("slack") {
        Some("slack")
    } else {
        None
    }
}

fn sanitize_source_segment(input: &str) -> String {
    let lowered = input.trim().to_ascii_lowercase();
    let mut sanitized = String::with_capacity(lowered.len());
    let mut last_was_separator = false;
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
            last_was_separator = false;
        } else {
            if !last_was_separator && !sanitized.is_empty() {
                sanitized.push('-');
                last_was_separator = true;
            }
        }
    }

    let sanitized = sanitized.trim_matches('-').to_string();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NodeSource, WorkerScopePolicy, platform_runtime_client_name, sanitize_source_segment,
    };
    use agent_protocol::TransportClientInfo;
    use std::path::Path;

    #[test]
    fn cli_client_maps_to_local_cli_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: "cloudagent-cli".to_string(),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "local:cli");
        assert_eq!(
            source.worker_scope_policy(),
            WorkerScopePolicy::WorkspaceScopedLocal
        );
    }

    #[test]
    fn web_client_maps_to_local_web_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: "cloudagent-web".to_string(),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "local:web");
        assert_eq!(
            source.worker_scope_policy(),
            WorkerScopePolicy::WorkspaceScopedLocal
        );
    }

    #[test]
    fn feishu_client_maps_to_distinct_im_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: platform_runtime_client_name("feishu"),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "im:feishu");
        assert_eq!(
            source.worker_scope_policy(),
            WorkerScopePolicy::DomainScopedShared
        );
    }

    #[test]
    fn arbitrary_platform_runtime_name_maps_to_its_own_im_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: platform_runtime_client_name("my-new-im"),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "im:my-new-im");
        assert_eq!(
            source.worker_scope_policy(),
            WorkerScopePolicy::DomainScopedShared
        );
    }

    #[test]
    fn unknown_remote_client_names_are_sanitized() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: "Third Party/Bridge v1".to_string(),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "remote:third-party-bridge-v1");
        assert_eq!(
            source.worker_scope_policy(),
            WorkerScopePolicy::DomainScopedShared
        );
    }

    #[test]
    fn placeholder_source_uses_sanitized_remote_scope() {
        let source = NodeSource::placeholder("127.0.0.1:47799#session 1");

        assert_eq!(source.domain_id(), "remote:127-0-0-1-47799-session-1");
        assert_eq!(
            source.worker_scope_key(None),
            "remote:127-0-0-1-47799-session-1"
        );
    }

    #[test]
    fn local_sources_derive_workspace_scoped_worker_key() {
        let source = NodeSource::from_domain_id("local:cli");

        let key = source.worker_scope_key(Some(Path::new("D:/Repo/App")));

        assert!(key.starts_with("local:cli@"));
    }

    #[test]
    fn local_sources_without_workspace_fall_back_to_domain_scope() {
        let source = NodeSource::from_domain_id("local:web");

        assert_eq!(source.worker_scope_key(None), "local:web");
    }

    #[test]
    fn remote_sources_keep_domain_scoped_worker_key() {
        let source = NodeSource::from_domain_id("im:feishu");

        assert_eq!(
            source.worker_scope_key(Some(Path::new("D:/Repo/App"))),
            "im:feishu"
        );
    }

    #[test]
    fn sanitize_source_segment_collapses_noise() {
        assert_eq!(
            sanitize_source_segment(" Node.Platform/Feishu "),
            "node-platform-feishu"
        );
        assert_eq!(sanitize_source_segment(""), "unknown");
    }
}
