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
) -> AdmissionDecision {
    if envelope.sender.sender_type.as_deref() != Some("user") {
        return AdmissionDecision::RejectedSenderType;
    }

    let is_group = matches!(envelope.message.chat_type.as_deref(), Some("group"));
    if !is_group || !group_only_mentioned {
        return AdmissionDecision::Admitted;
    }

    if mentions_bot(envelope, bot) {
        AdmissionDecision::Admitted
    } else {
        AdmissionDecision::RejectedGroupNotMentioned
    }
}

pub fn mentions_bot(envelope: &FeishuMessageEnvelope, bot: &FeishuBotIdentity) -> bool {
    let mentions = envelope.message.mentions.as_deref().unwrap_or(&[]);
    if mentions.is_empty() {
        return false;
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
