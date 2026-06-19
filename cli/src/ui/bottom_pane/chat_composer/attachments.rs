use std::path::PathBuf;

use agent_core::conversation::{AttachmentRef, InputItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LocalAttachedImage {
    pub(super) placeholder: String,
    pub(super) path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RemoteAttachedImage {
    pub(super) placeholder: String,
    pub(super) url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AttachedSkill {
    pub(super) placeholder: String,
    pub(super) name: String,
    pub(super) path: String,
}

enum PendingImage<'a> {
    Local(&'a LocalAttachedImage),
    Remote(&'a RemoteAttachedImage),
}

impl PendingImage<'_> {
    fn placeholder(&self) -> &str {
        match self {
            Self::Local(image) => &image.placeholder,
            Self::Remote(image) => &image.placeholder,
        }
    }

    fn to_input_item(&self) -> InputItem {
        match self {
            Self::Local(image) => InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: image.path.display().to_string(),
                },
                detail: None,
                alt: None,
            },
            Self::Remote(image) => InputItem::Image {
                source: AttachmentRef::RemoteUrl {
                    url: image.url.clone(),
                },
                detail: None,
                alt: None,
            },
        }
    }
}

pub(super) fn build_submission_content(
    text: &str,
    local_images: &[LocalAttachedImage],
    remote_images: &[RemoteAttachedImage],
    attached_skills: &[AttachedSkill],
) -> Vec<InputItem> {
    let mut content = Vec::new();
    let mut remaining = text;
    let mut images = local_images
        .iter()
        .map(PendingImage::Local)
        .chain(remote_images.iter().map(PendingImage::Remote))
        .filter(|image| text.contains(image.placeholder()))
        .collect::<Vec<_>>();
    let mut skills = attached_skills
        .iter()
        .filter(|skill| text.contains(&skill.placeholder))
        .collect::<Vec<_>>();

    while !images.is_empty() || !skills.is_empty() {
        let next_image = images.iter().enumerate().filter_map(|(idx, image)| {
            remaining
                .find(image.placeholder())
                .map(|offset| (idx, offset))
        });
        let next_skill = skills.iter().enumerate().filter_map(|(idx, skill)| {
            remaining
                .find(&skill.placeholder)
                .map(|offset| (idx, offset))
        });
        let next = next_image
            .map(|(idx, offset)| (PendingSubmissionItem::Image(idx), offset))
            .chain(next_skill.map(|(idx, offset)| (PendingSubmissionItem::Skill(idx), offset)))
            .min_by_key(|(_, offset)| *offset);

        let Some((item, offset)) = next else {
            break;
        };
        let (before, rest) = remaining.split_at(offset);
        push_text_item(&mut content, before);
        match item {
            PendingSubmissionItem::Image(image_idx) => {
                let image = images.remove(image_idx);
                content.push(image.to_input_item());
                remaining = &rest[image.placeholder().len()..];
            }
            PendingSubmissionItem::Skill(skill_idx) => {
                let skill = skills.remove(skill_idx);
                content.push(InputItem::Skill {
                    name: skill.name.clone(),
                    path: skill.path.clone(),
                });
                remaining = &rest[skill.placeholder.len()..];
            }
        }
    }

    push_text_item(&mut content, remaining);
    content
}

enum PendingSubmissionItem {
    Image(usize),
    Skill(usize),
}

fn push_text_item(content: &mut Vec<InputItem>, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    content.push(InputItem::Text {
        text: text.trim().to_string(),
    });
}
