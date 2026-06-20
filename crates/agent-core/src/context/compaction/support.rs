use crate::conversation::{AttachmentRef, ImageDetail, InputItem, ResponseItem};

pub(super) fn render_input_items_for_compaction(items: &[InputItem]) -> String {
    items
        .iter()
        .map(render_input_item_for_compaction)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn single_line(value: &str) -> String {
    let compact = value.replace(['\n', '\r'], " ");
    let trimmed = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = trimmed.chars();
    let snippet = chars.by_ref().take(220).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}...")
    } else {
        snippet
    }
}

pub(super) fn estimate_text_tokens(text: &str) -> usize {
    text.chars().count().saturating_div(3).max(1)
}

pub(super) fn estimate_message_tokens(messages: &[ResponseItem]) -> usize {
    let chars = messages
        .iter()
        .map(|item| match item {
            ResponseItem::System { content } => content.len(),
            ResponseItem::User { content } => render_input_items_for_compaction(content).len(),
            ResponseItem::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let text_len = content.as_ref().map_or(0, String::len);
                let tool_len: usize = tool_calls
                    .iter()
                    .map(|call| call.name.len() + call.arguments.to_string().len())
                    .sum();
                text_len + tool_len
            }
            ResponseItem::Tool { name, content, .. } => name.len() + content.len(),
        })
        .sum::<usize>();

    chars.saturating_div(3).max(1)
}

fn render_input_item_for_compaction(item: &InputItem) -> String {
    match item {
        InputItem::Text { text } => text.clone(),
        InputItem::Image {
            source,
            detail,
            alt,
        } => {
            let mut parts = vec!["[image".to_string()];
            if let Some(alt) = alt.as_ref().filter(|alt| !alt.trim().is_empty()) {
                parts.push(format!("alt={}", single_line(alt)));
            }
            if let Some(detail) = detail {
                parts.push(format!("detail={}", render_image_detail(detail)));
            }
            parts.push(format!("source={}", render_attachment_ref(source)));
            format!("{}]", parts.join(" "))
        }
        InputItem::File {
            source,
            mime_type,
            name,
        } => {
            let mut parts = vec!["[file".to_string()];
            if let Some(name) = name.as_ref().filter(|name| !name.trim().is_empty()) {
                parts.push(format!("name={}", single_line(name)));
            }
            if let Some(mime_type) = mime_type
                .as_ref()
                .filter(|mime_type| !mime_type.trim().is_empty())
            {
                parts.push(format!("mime={mime_type}"));
            }
            parts.push(format!("source={}", render_attachment_ref(source)));
            format!("{}]", parts.join(" "))
        }
        InputItem::Mention { name, path } => {
            format!("[mention @{name} path={}]", single_line(path))
        }
        InputItem::Skill { name, path } => format!("[skill ${name} path={}]", single_line(path)),
    }
}

fn render_attachment_ref(source: &AttachmentRef) -> String {
    match source {
        AttachmentRef::InlineDataUrl { data_url } => {
            let prefix = data_url.split(',').next().unwrap_or("data:unknown");
            format!("{prefix},...")
        }
        AttachmentRef::RemoteUrl { url } => single_line(url),
        AttachmentRef::HubAsset {
            asset_id,
            download_url,
        } => match download_url {
            Some(download_url) => format!("hub:{asset_id} ({})", single_line(download_url)),
            None => format!("hub:{asset_id}"),
        },
        AttachmentRef::LocalPath { path } => single_line(path),
    }
}

fn render_image_detail(detail: &ImageDetail) -> &'static str {
    match detail {
        ImageDetail::Auto => "auto",
        ImageDetail::Low => "low",
        ImageDetail::High => "high",
        ImageDetail::Original => "original",
    }
}
