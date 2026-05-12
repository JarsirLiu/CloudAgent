use super::types::{FeishuBotIdentity, FeishuMessageEnvelope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Admitted,
    RejectedSenderType,
    RejectedGroupNotMentioned,
}

pub fn evaluate_admission(
    envelope: &FeishuMessageEnvelope,
    bot: &FeishuBotIdentity,
    group_only_mentioned: bool,
    group_reply_without_mention: bool,
    replied_to_known_bot_message: bool,
) -> AdmissionDecision {
    if envelope.sender.sender_type.as_deref() != Some("user") {
        return AdmissionDecision::RejectedSenderType;
    }

    let is_group = matches!(envelope.message.chat_type.as_deref(), Some("group"));
    if !is_group || !group_only_mentioned {
        return AdmissionDecision::Admitted;
    }

    if mentions_bot(envelope, bot)
        || (group_reply_without_mention && is_reply_chain(envelope) && replied_to_known_bot_message)
    {
        AdmissionDecision::Admitted
    } else {
        AdmissionDecision::RejectedGroupNotMentioned
    }
}

fn is_reply_chain(envelope: &FeishuMessageEnvelope) -> bool {
    envelope
        .message
        .root_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || envelope
            .message
            .parent_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

pub fn mentions_bot(envelope: &FeishuMessageEnvelope, bot: &FeishuBotIdentity) -> bool {
    let mentions = envelope.message.mentions.as_deref().unwrap_or(&[]);
    if mentions.is_empty() {
        return false;
    }

    if bot.open_id.trim().is_empty() && bot.name.trim().is_empty() {
        return true;
    }

    mentions.iter().any(|mention| {
        let open_id_matches = mention
            .id
            .as_ref()
            .and_then(|id| id.open_id.as_deref())
            .map(|open_id| !bot.open_id.is_empty() && open_id == bot.open_id)
            .unwrap_or(false);

        let name_matches = mention
            .name
            .as_deref()
            .map(|name| !bot.name.is_empty() && name == bot.name)
            .unwrap_or(false);

        open_id_matches || name_matches
    })
}

#[cfg(test)]
mod tests {
    use super::{AdmissionDecision, evaluate_admission};
    use crate::adapter::feishu::types::{
        FeishuBotIdentity, FeishuMention, FeishuMessage, FeishuMessageEnvelope, FeishuSender,
        FeishuUserId,
    };

    fn envelope() -> FeishuMessageEnvelope {
        FeishuMessageEnvelope {
            sender: FeishuSender {
                sender_id: Some(FeishuUserId {
                    open_id: Some("ou_user".to_string()),
                    user_id: None,
                    union_id: None,
                }),
                sender_type: Some("user".to_string()),
                tenant_key: None,
            },
            message: FeishuMessage {
                message_id: Some("om_1".to_string()),
                root_id: None,
                parent_id: None,
                chat_id: Some("oc_group".to_string()),
                chat_type: Some("group".to_string()),
                message_type: Some("text".to_string()),
                content: Some("{\"text\":\"hello\"}".to_string()),
                mentions: None,
            },
            create_time: None,
        }
    }

    #[test]
    fn group_reply_chain_can_continue_without_repeat_mention() {
        let mut env = envelope();
        env.message.parent_id = Some("om_parent".to_string());

        let decision = evaluate_admission(
            &env,
            &FeishuBotIdentity {
                open_id: "ou_bot".to_string(),
                name: "cloudagent".to_string(),
            },
            true,
            true,
            true,
        );

        assert_eq!(decision, AdmissionDecision::Admitted);
    }

    #[test]
    fn group_message_without_mention_or_reply_chain_is_rejected() {
        let decision = evaluate_admission(
            &envelope(),
            &FeishuBotIdentity {
                open_id: "ou_bot".to_string(),
                name: "cloudagent".to_string(),
            },
            true,
            true,
            false,
        );

        assert_eq!(decision, AdmissionDecision::RejectedGroupNotMentioned);
    }

    #[test]
    fn explicit_bot_mention_is_admitted() {
        let mut env = envelope();
        env.message.mentions = Some(vec![FeishuMention {
            key: Some("@_user_1".to_string()),
            name: Some("cloudagent".to_string()),
            id: Some(FeishuUserId {
                open_id: Some("ou_bot".to_string()),
                user_id: None,
                union_id: None,
            }),
        }]);

        let decision = evaluate_admission(
            &env,
            &FeishuBotIdentity {
                open_id: "ou_bot".to_string(),
                name: "cloudagent".to_string(),
            },
            true,
            false,
            false,
        );

        assert_eq!(decision, AdmissionDecision::Admitted);
    }

    #[test]
    fn explicit_mention_is_admitted_even_if_bot_identity_is_empty() {
        let mut env = envelope();
        env.message.mentions = Some(vec![FeishuMention {
            key: Some("@_user_1".to_string()),
            name: Some("cloudagent".to_string()),
            id: Some(FeishuUserId {
                open_id: Some("ou_unknown".to_string()),
                user_id: None,
                union_id: None,
            }),
        }]);

        let decision = evaluate_admission(&env, &FeishuBotIdentity::default(), true, false, false);

        assert_eq!(decision, AdmissionDecision::Admitted);
    }
}
