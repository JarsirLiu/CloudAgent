use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use agent_app_server_client::AppServerClient;
use anyhow::Result;

#[path = "local_basic_actions.rs"]
mod local_basic_actions;
#[path = "local_command_actions.rs"]
mod local_command_actions;
#[path = "local_gateway_actions.rs"]
mod local_gateway_actions;
#[path = "local_session_actions.rs"]
mod local_session_actions;
#[path = "local_skill_actions.rs"]
mod local_skill_actions;
#[path = "local_workspace_actions.rs"]
mod local_workspace_actions;

pub(crate) async fn handle_tui_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalCopy
        | ParsedInput::LocalCopyText(_)
        | ParsedInput::LocalImagePaste
        | ParsedInput::LocalHelp
        | ParsedInput::LocalInputError(_) => {
            local_basic_actions::handle_basic_input(app, input).await
        }
        ParsedInput::LocalPermissionMode(_)
        | ParsedInput::LocalConfig { .. }
        | ParsedInput::LocalReasoning(_)
        | ParsedInput::LocalModel(_)
        | ParsedInput::LocalFilterToggle(_) => {
            local_workspace_actions::handle_workspace_input(app, client, input).await
        }
        ParsedInput::LocalSkillInsert(_) | ParsedInput::LocalSkillsOpen => {
            local_skill_actions::handle_skill_input(app, client, input).await
        }
        ParsedInput::LocalGatewayOpen
        | ParsedInput::LocalGatewaySelect(_)
        | ParsedInput::LocalGatewayWeixinLoginStart(_)
        | ParsedInput::LocalGatewayWeixinLoginCheck { .. }
        | ParsedInput::LocalGatewaySave { .. } => {
            local_gateway_actions::handle_gateway_input(app, client, input).await
        }
        ParsedInput::LocalConversationCreate(_)
        | ParsedInput::LocalSessionListNextPage { .. }
        | ParsedInput::LocalConversationSwitch(_)
        | ParsedInput::LocalConversationTitle(_)
        | ParsedInput::LocalConversationArchive(_)
        | ParsedInput::LocalConversationDelete(_) => {
            local_session_actions::handle_session_input(app, client, input).await
        }
        ParsedInput::Command(_) | ParsedInput::ServerRequestAnswer { .. } => {
            local_command_actions::handle_command_input(app, client, input).await
        }
    }
}
