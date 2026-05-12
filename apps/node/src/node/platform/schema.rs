use super::config_store::PlatformConfigState;
use agent_gateway::adapter::{feishu, wecom, weixin};
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
    let fields = editable_specs_for(platform)
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
        "weixin" => build_weixin_config(state)?.validate(),
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
        group_only_mentioned: optional_bool_value(&values, "group_only_mentioned").unwrap_or(true),
        group_reply_without_mention: optional_bool_value(&values, "group_reply_without_mention")
            .unwrap_or(true),
        ..Default::default()
    })
}

pub(crate) fn build_wecom_config(state: &PlatformConfigState) -> Result<wecom::WecomAdapterConfig> {
    let values = merged_values("wecom", state);
    Ok(wecom::WecomAdapterConfig {
        bot_id: required_value(&values, "bot_id", "CLOUDAGENT_WECOM_BOT_ID")?,
        bot_secret: required_value(&values, "bot_secret", "CLOUDAGENT_WECOM_BOT_SECRET")?,
        dm_policy: optional_policy_value(&values, "dm_policy").unwrap_or_default(),
        group_policy: optional_policy_value(&values, "group_policy").unwrap_or_default(),
        allow_from: optional_list_value(&values, "allow_from"),
        group_allow_from: optional_list_value(&values, "group_allow_from"),
    })
}

pub(crate) fn build_weixin_config(
    state: &PlatformConfigState,
) -> Result<weixin::WeixinAdapterConfig> {
    let values = merged_values("weixin", state);
    Ok(weixin::WeixinAdapterConfig {
        account_id: required_value(&values, "account_id", "CLOUDAGENT_WEIXIN_ACCOUNT_ID")?,
        token: required_value(&values, "token", "CLOUDAGENT_WEIXIN_TOKEN")?,
        base_url: optional_value(&values, "base_url")
            .unwrap_or_else(|| "https://ilinkai.weixin.qq.com".to_string()),
    })
}

pub(crate) fn editable_specs_for(platform: &str) -> &'static [PlatformFieldSpec] {
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

pub(crate) fn supported_specs_for(platform: &str) -> &'static [PlatformFieldSpec] {
    match platform {
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
            PlatformFieldSpec {
                key: "dm_policy",
                required: false,
                secret: false,
            },
            PlatformFieldSpec {
                key: "group_policy",
                required: false,
                secret: false,
            },
            PlatformFieldSpec {
                key: "allow_from",
                required: false,
                secret: false,
            },
            PlatformFieldSpec {
                key: "group_allow_from",
                required: false,
                secret: false,
            },
        ],
        "weixin" => &[
            PlatformFieldSpec {
                key: "account_id",
                required: true,
                secret: false,
            },
            PlatformFieldSpec {
                key: "token",
                required: true,
                secret: true,
            },
            PlatformFieldSpec {
                key: "base_url",
                required: false,
                secret: false,
            },
        ],
        _ => editable_specs_for(platform),
    }
}

fn merged_values(platform: &str, state: &PlatformConfigState) -> BTreeMap<String, String> {
    let mut merged = state.platforms.get(platform).cloned().unwrap_or_default();
    for spec in supported_specs_for(platform) {
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

fn optional_list_value(values: &BTreeMap<String, String>, key: &str) -> Vec<String> {
    values
        .get(key)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn optional_policy_value(
    values: &BTreeMap<String, String>,
    key: &str,
) -> Option<wecom::WecomPolicy> {
    values
        .get(key)
        .and_then(|value| wecom::WecomPolicy::parse(value))
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
        ("feishu", "group_only_mentioned") => Some("CLOUDAGENT_FEISHU_GROUP_ONLY_MENTIONED"),
        ("feishu", "group_reply_without_mention") => {
            Some("CLOUDAGENT_FEISHU_GROUP_REPLY_WITHOUT_MENTION")
        }
        ("wecom", "bot_id") => Some("CLOUDAGENT_WECOM_BOT_ID"),
        ("wecom", "bot_secret") => Some("CLOUDAGENT_WECOM_BOT_SECRET"),
        ("wecom", "dm_policy") => Some("CLOUDAGENT_WECOM_DM_POLICY"),
        ("wecom", "group_policy") => Some("CLOUDAGENT_WECOM_GROUP_POLICY"),
        ("wecom", "allow_from") => Some("CLOUDAGENT_WECOM_ALLOW_FROM"),
        ("wecom", "group_allow_from") => Some("CLOUDAGENT_WECOM_GROUP_ALLOW_FROM"),
        ("weixin", "account_id") => Some("CLOUDAGENT_WEIXIN_ACCOUNT_ID"),
        ("weixin", "token") => Some("CLOUDAGENT_WEIXIN_TOKEN"),
        ("weixin", "base_url") => Some("CLOUDAGENT_WEIXIN_BASE_URL"),
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
    use super::{
        PlatformConfigState, build_feishu_config, build_wecom_config, build_weixin_config,
        parse_bool_value,
    };
    use agent_gateway::adapter::wecom::WecomPolicy;
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
        platform_values.insert("group_only_mentioned".to_string(), "false".to_string());
        platform_values.insert(
            "group_reply_without_mention".to_string(),
            "false".to_string(),
        );

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
        assert!(!config.group_only_mentioned);
        assert!(!config.group_reply_without_mention);
    }

    #[test]
    fn build_wecom_config_reads_policy_fields() {
        let mut platform_values = BTreeMap::new();
        platform_values.insert("bot_id".to_string(), "bot_xxx".to_string());
        platform_values.insert("bot_secret".to_string(), "sec_xxx".to_string());
        platform_values.insert("dm_policy".to_string(), "allowlist".to_string());
        platform_values.insert("group_policy".to_string(), "disabled".to_string());
        platform_values.insert("allow_from".to_string(), "user1,user2".to_string());
        platform_values.insert("group_allow_from".to_string(), "chat1,chat2".to_string());

        let mut platforms = BTreeMap::new();
        platforms.insert("wecom".to_string(), platform_values);

        let state = PlatformConfigState {
            version: 1,
            platforms,
        };

        let config = build_wecom_config(&state).expect("config");
        assert_eq!(config.dm_policy, WecomPolicy::Allowlist);
        assert_eq!(config.group_policy, WecomPolicy::Disabled);
        assert_eq!(
            config.allow_from,
            vec!["user1".to_string(), "user2".to_string()]
        );
        assert_eq!(
            config.group_allow_from,
            vec!["chat1".to_string(), "chat2".to_string()]
        );
    }

    #[test]
    fn wecom_editable_specs_only_show_required_fields() {
        let keys = super::editable_specs_for("wecom")
            .iter()
            .map(|spec| spec.key)
            .collect::<Vec<_>>();
        assert_eq!(keys, vec!["bot_id", "bot_secret"]);

        let supported = super::supported_specs_for("wecom")
            .iter()
            .map(|spec| spec.key)
            .collect::<Vec<_>>();
        assert!(supported.contains(&"dm_policy"));
        assert!(supported.contains(&"group_policy"));
        assert!(supported.contains(&"allow_from"));
        assert!(supported.contains(&"group_allow_from"));
    }

    #[test]
    fn build_weixin_config_reads_required_fields() {
        let mut platform_values = BTreeMap::new();
        platform_values.insert("account_id".to_string(), "acct_1".to_string());
        platform_values.insert("token".to_string(), "token_1".to_string());

        let mut platforms = BTreeMap::new();
        platforms.insert("weixin".to_string(), platform_values);

        let state = PlatformConfigState {
            version: 1,
            platforms,
        };

        let config = build_weixin_config(&state).expect("config");
        assert_eq!(config.account_id, "acct_1");
        assert_eq!(config.token, "token_1");
        assert_eq!(config.base_url, "https://ilinkai.weixin.qq.com");
    }

    #[test]
    fn weixin_editable_specs_hide_manual_credentials() {
        let keys = super::editable_specs_for("weixin")
            .iter()
            .map(|spec| spec.key)
            .collect::<Vec<_>>();
        assert!(keys.is_empty());

        let supported = super::supported_specs_for("weixin")
            .iter()
            .map(|spec| spec.key)
            .collect::<Vec<_>>();
        assert!(supported.contains(&"base_url"));
    }
}
