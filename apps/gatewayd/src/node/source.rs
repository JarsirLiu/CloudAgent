use agent_protocol::TransportClientInfo;

const PLATFORM_RUNTIME_CLIENT_PREFIX: &str = "gatewayd-platform-";
const PLATFORM_RUNTIME_FALLBACK_PREFIX: &str = "cloudagent-platform-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodeSource {
    domain_id: String,
    worker_scope_key: String,
}

impl NodeSource {
    pub(crate) fn placeholder(session_scope_key: impl Into<String>) -> Self {
        let session_scope_key = sanitize_source_segment(&session_scope_key.into());
        Self {
            domain_id: format!("remote:{session_scope_key}"),
            worker_scope_key: format!("remote:{session_scope_key}"),
        }
    }

    pub(crate) fn from_client_info(client_info: &TransportClientInfo) -> Self {
        let domain_id = source_domain_id(&client_info.name);
        Self {
            worker_scope_key: domain_id.clone(),
            domain_id,
        }
    }

    pub(crate) fn worker_scope_key(&self) -> &str {
        &self.worker_scope_key
    }

    pub(crate) fn domain_id(&self) -> &str {
        &self.domain_id
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
    use super::{NodeSource, platform_runtime_client_name, sanitize_source_segment};
    use agent_protocol::TransportClientInfo;

    #[test]
    fn cli_client_maps_to_local_cli_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: "cloudagent-cli".to_string(),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "local:cli");
        assert_eq!(source.worker_scope_key(), "local:cli");
    }

    #[test]
    fn web_client_maps_to_local_web_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: "cloudagent-web".to_string(),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "local:web");
        assert_eq!(source.worker_scope_key(), "local:web");
    }

    #[test]
    fn feishu_client_maps_to_distinct_im_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: platform_runtime_client_name("feishu"),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "im:feishu");
        assert_eq!(source.worker_scope_key(), "im:feishu");
    }

    #[test]
    fn arbitrary_platform_runtime_name_maps_to_its_own_im_domain() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: platform_runtime_client_name("my-new-im"),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "im:my-new-im");
        assert_eq!(source.worker_scope_key(), "im:my-new-im");
    }

    #[test]
    fn unknown_remote_client_names_are_sanitized() {
        let source = NodeSource::from_client_info(&TransportClientInfo {
            name: "Third Party/Bridge v1".to_string(),
            version: "0.1.0".to_string(),
        });

        assert_eq!(source.domain_id(), "remote:third-party-bridge-v1");
        assert_eq!(source.worker_scope_key(), "remote:third-party-bridge-v1");
    }

    #[test]
    fn placeholder_source_uses_sanitized_remote_scope() {
        let source = NodeSource::placeholder("127.0.0.1:47799#session 1");

        assert_eq!(source.domain_id(), "remote:127-0-0-1-47799-session-1");
        assert_eq!(
            source.worker_scope_key(),
            "remote:127-0-0-1-47799-session-1"
        );
    }

    #[test]
    fn sanitize_source_segment_collapses_noise() {
        assert_eq!(
            sanitize_source_segment(" GatewayD.Platform/Feishu "),
            "gatewayd-platform-feishu"
        );
        assert_eq!(sanitize_source_segment(""), "unknown");
    }
}
