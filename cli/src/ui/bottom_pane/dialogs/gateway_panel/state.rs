use super::WeixinLoginSessionView;
use crate::input::intent::GatewayConfigUpdate;
use crate::ui::bottom_pane::support::form_text_field::FormTextField;
use agent_protocol::{PlatformConfigField, PlatformControlEntry};

pub(crate) enum GatewayPanelMode {
    List {
        entries: Vec<PlatformControlEntry>,
        selected: usize,
    },
    Edit {
        platform: String,
        enabled: bool,
        configured: bool,
        selected: usize,
        fields: Vec<EditableField>,
        weixin_login: Option<WeixinLoginSessionView>,
    },
}

pub(crate) struct EditableField {
    pub(crate) key: String,
    pub(crate) input: FormTextField,
    pub(crate) required: bool,
    pub(crate) is_secret: bool,
    pub(crate) was_set: bool,
}

impl EditableField {
    pub(crate) fn new(field: PlatformConfigField) -> Self {
        Self {
            key: field.key,
            input: FormTextField::new(field.value.unwrap_or_default()),
            required: field.required,
            is_secret: field.is_secret,
            was_set: field.is_set,
        }
    }
}

impl GatewayPanelMode {
    pub(crate) fn collect_updates(fields: &[EditableField]) -> Vec<GatewayConfigUpdate> {
        fields
            .iter()
            .filter(|field| field.input.is_dirty())
            .map(|field| GatewayConfigUpdate {
                key: field.key.clone(),
                value: field.input.trimmed_value(),
            })
            .collect()
    }
}
