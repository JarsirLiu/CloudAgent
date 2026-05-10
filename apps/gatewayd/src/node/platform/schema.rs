use super::config_store::PlatformConfigState;
use agent_gateway::adapter::{feishu, wecom};
use agent_protocol::{PlatformConfigField, PlatformConfigResponse};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) struct PlatformFieldSpec {
    pub(crate) key: &'static str,
    pub(crate) required: bool,
    pub(crate) secret: bool,
}

pub(crate) fn config_response(
    state: &PlatformConfigState,
    platform: &str,
) -> PlatformConfigResponse {
    let values = state.platforms.get(platform);
    let fields = specs_for(platform)
        .iter()
        .map(|spec| {
            let raw = values.and_then(|map| map.get(spec.key));
            PlatformConfigField {
                key: spec.key.to_string(),
                value: raw.map(|value| display_value(value, spec.secret)),
                is_secret: spec.secret,
                is_set: raw.is_some(),
                required: spec.required,
            }
        })
        .collect();
    PlatformConfigResponse {
        platform: platform.to_string(),
        configured: validate_platform_config(platform, state).is_ok(),
        fields,
    }
}

pub(crate) fn validate_platform_config(platform: &str, state: &PlatformConfigState) -> Result<()> {
    match platform {
        "feishu" => build_feishu_config(state)?.validate(),
        "wecom" => build_wecom_config(state)?.validate(),
        "weixin" => Ok(()),
        other => anyhow::bail!("unsupported platform `{other}`"),
    }
}

pub(crate) fn build_feishu_config(
    state: &PlatformConfigState,
) -> Result<feishu::FeishuAdapterConfig> {
    let values = merged_values("feishu", state);
    Ok(feishu::FeishuAdapterConfig {
        app_id: required_value(&values, "app_id", "CLOUDAGENT_FEISHU_APP_ID")?,
        app_secret: required_value(&values, "app_secret", "CLOUDAGENT_FEISHU_APP_SECRET")?,
        domain: optional_value(&values, "domain")
            .unwrap_or_else(|| "https://open.feishu.cn".to_string()),
        enable_cards: optional_bool_value(&values, "enable_cards").unwrap_or(true),
        thread_isolation: optional_bool_value(&values, "thread_isolation").unwrap_or(true),
        reply_to_trigger: optional_bool_value(&values, "reply_to_trigger").unwrap_or(true),
        ..Default::default()
    })
}

pub(crate) fn build_wecom_config(state: &PlatformConfigState) -> Result<wecom::WecomAdapterConfig> {
    let values = merged_values("wecom", state);
    Ok(wecom::WecomAdapterConfig {
        bot_id: required_value(&values, "bot_id", "CLOUDAGENT_WECOM_BOT_ID")?,
        bot_secret: required_value(&values, "bot_secret", "CLOUDAGENT_WECOM_BOT_SECRET")?,
    })
}

pub(crate) fn specs_for(platform: &str) -> &'static [PlatformFieldSpec] {
    match platform {
        "feishu" => &[
            PlatformFieldSpec {
                key: "app_id",
                required: true,
                secret: false,
            },
            PlatformFieldSpec {
                key: "app_secret",
                required: true,
                secret: true,
            },
            PlatformFieldSpec {
                key: "domain",
                required: false,
                secret: false,
            },
            PlatformFieldSpec {
                key: "enable_cards",
                required: false,
                secret: false,
            },
            PlatformFieldSpec {
                key: "thread_isolation",
                required: false,
                secret: false,
            },
            PlatformFieldSpec {
                key: "reply_to_trigger",
                required: false,
                secret: false,
            },
        ],
        "wecom" => &[
            PlatformFieldSpec {
                key: "bot_id",
                required: true,
                secret: false,
            },
            PlatformFieldSpec {
                key: "bot_secret",
                required: true,
                secret: true,
            },
        ],
        "weixin" => &[],
        _ => &[],
    }
}

fn merged_values(platform: &str, state: &PlatformConfigState) -> BTreeMap<String, String> {
    let mut merged = state.platforms.get(platform).cloned().unwrap_or_default();
    for spec in specs_for(platform) {
        if merged.contains_key(spec.key) {
            continue;
        }
        if let Some(env_name) = env_name(platform, spec.key)
            && let Ok(value) = std::env::var(env_name)
            && !value.trim().is_empty()
        {
            merged.insert(spec.key.to_string(), value);
        }
    }
    merged
}

fn required_value(values: &BTreeMap<String, String>, key: &str, env_name: &str) -> Result<String> {
    values
        .get(key)
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing {key} (or {env_name})"))
}

fn optional_value(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .cloned()
        .filter(|value| !value.trim().is_empty())
}

fn optional_bool_value(values: &BTreeMap<String, String>, key: &str) -> Option<bool> {
    values.get(key).and_then(|value| parse_bool_value(value))
}

fn parse_bool_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_name(platform: &str, key: &str) -> Option<&'static str> {
    match (platform, key) {
        ("feishu", "app_id") => Some("CLOUDAGENT_FEISHU_APP_ID"),
        ("feishu", "app_secret") => Some("CLOUDAGENT_FEISHU_APP_SECRET"),
        ("feishu", "domain") => Some("CLOUDAGENT_FEISHU_DOMAIN"),
        ("feishu", "enable_cards") => Some("CLOUDAGENT_FEISHU_ENABLE_CARDS"),
        ("feishu", "thread_isolation") => Some("CLOUDAGENT_FEISHU_THREAD_ISOLATION"),
        ("feishu", "reply_to_trigger") => Some("CLOUDAGENT_FEISHU_REPLY_TO_TRIGGER"),
        ("wecom", "bot_id") => Some("CLOUDAGENT_WECOM_BOT_ID"),
        ("wecom", "bot_secret") => Some("CLOUDAGENT_WECOM_BOT_SECRET"),
        _ => None,
    }
}

fn display_value(value: &str, secret: bool) -> String {
    if !secret {
        return value.to_string();
    }
    mask_secret(value)
}

fn mask_secret(value: &str) -> String {
    let count = value.chars().count();
    if count <= 4 {
        return "*".repeat(count.max(1));
    }
    let suffix = value.chars().skip(count - 4).collect::<String>();
    format!("{}{}", "*".repeat(count - 4), suffix)
}

#[cfg(test)]
mod tests {
    use super::{PlatformConfigState, build_feishu_config, parse_bool_value};
    use std::collections::BTreeMap;

    #[test]
    fn parse_bool_value_accepts_common_spellings() {
        assert_eq!(parse_bool_value("true"), Some(true));
        assert_eq!(parse_bool_value("YES"), Some(true));
        assert_eq!(parse_bool_value("0"), Some(false));
        assert_eq!(parse_bool_value("off"), Some(false));
        assert_eq!(parse_bool_value("maybe"), None);
    }

    #[test]
    fn build_feishu_config_reads_bool_flags() {
        let mut platform_values = BTreeMap::new();
        platform_values.insert("app_id".to_string(), "cli_xxx".to_string());
        platform_values.insert("app_secret".to_string(), "sec_xxx".to_string());
        platform_values.insert("enable_cards".to_string(), "false".to_string());
        platform_values.insert("thread_isolation".to_string(), "false".to_string());
        platform_values.insert("reply_to_trigger".to_string(), "false".to_string());

        let mut platforms = BTreeMap::new();
        platforms.insert("feishu".to_string(), platform_values);

        let state = PlatformConfigState {
            version: 1,
            platforms,
        };

        let config = build_feishu_config(&state).expect("config");
        assert!(!config.enable_cards);
        assert!(!config.thread_isolation);
        assert!(!config.reply_to_trigger);
    }
}
