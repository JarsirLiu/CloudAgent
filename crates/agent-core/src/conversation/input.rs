use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttachmentRef {
    InlineDataUrl {
        data_url: String,
    },
    RemoteUrl {
        url: String,
    },
    HubAsset {
        asset_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        download_url: Option<String>,
    },
    LocalPath {
        path: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    Text {
        text: String,
    },
    Image {
        source: AttachmentRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    File {
        source: AttachmentRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Mention {
        name: String,
        path: String,
    },
    Skill {
        name: String,
        path: String,
    },
}

impl InputItem {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn display_text(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::Image { alt, .. } => alt
                .as_ref()
                .map(|alt| format!("[image: {alt}]"))
                .unwrap_or_else(|| "[image]".to_string()),
            Self::File { name, .. } => name
                .as_ref()
                .map(|name| format!("[file: {name}]"))
                .unwrap_or_else(|| "[file]".to_string()),
            Self::Mention { name, .. } => format!("@{name}"),
            Self::Skill { name, .. } => format!("${name}"),
        }
    }
}

pub fn input_items_attachment_count(items: &[InputItem]) -> usize {
    items
        .iter()
        .filter(|item| matches!(item, InputItem::Image { .. } | InputItem::File { .. }))
        .count()
}

pub fn input_items_preview_text(items: &[InputItem], max_chars: usize) -> String {
    let preview = input_items_display_text(items)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if preview.chars().count() <= max_chars {
        return preview;
    }

    let mut out = String::new();
    for (idx, ch) in preview.chars().enumerate() {
        if idx >= max_chars.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

pub fn input_items_display_text(items: &[InputItem]) -> String {
    items
        .iter()
        .map(InputItem::display_text)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn input_items_to_plain_text(items: &[InputItem]) -> String {
    input_items_display_text(items)
}

pub fn input_items_text_len(items: &[InputItem]) -> usize {
    input_items_to_plain_text(items).chars().count()
}

pub fn input_items_are_blank(items: &[InputItem]) -> bool {
    input_items_to_plain_text(items).trim().is_empty()
}

pub fn text_input_items(text: impl Into<String>) -> Vec<InputItem> {
    vec![InputItem::text(text)]
}
