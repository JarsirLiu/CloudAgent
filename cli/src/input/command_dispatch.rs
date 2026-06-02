use crate::input::intent::ComposerIntent;
use crate::input::slash_command::SlashCommand;

pub(crate) fn intent_for_slash_command(command: SlashCommand, args: &str) -> ComposerIntent {
    match command {
        SlashCommand::Clear => ComposerIntent::Reset,
        SlashCommand::Compact => ComposerIntent::Compact,
        SlashCommand::Copy => ComposerIntent::Copy,
        SlashCommand::Help => ComposerIntent::Help,
        SlashCommand::Interrupt => ComposerIntent::Interrupt,
        SlashCommand::Session => {
            let trimmed = args.trim();
            if trimmed.is_empty() {
                ComposerIntent::Session
            } else {
                ComposerIntent::SessionSwitch(trimmed.to_string())
            }
        }
        SlashCommand::NewConversation => ComposerIntent::NewConversation(args.trim().to_string()),
        SlashCommand::SetTitle => ComposerIntent::SetTitle(args.trim().to_string()),
        SlashCommand::ArchiveConversation => {
            ComposerIntent::ArchiveConversation(args.trim().to_string())
        }
        SlashCommand::DeleteConversation => {
            ComposerIntent::DeleteConversation(args.trim().to_string())
        }
        SlashCommand::Filter => ComposerIntent::Filter(args.trim().to_string()),
        SlashCommand::Permissions => ComposerIntent::Permissions(args.trim().to_string()),
        SlashCommand::Config => ComposerIntent::Config,
        SlashCommand::Reasoning => ComposerIntent::Reasoning(args.trim().to_string()),
        SlashCommand::Model => ComposerIntent::Model(args.trim().to_string()),
        SlashCommand::Skill => ComposerIntent::Skill(args.trim().to_string()),
        SlashCommand::Skills => ComposerIntent::Skills,
        SlashCommand::Gateway => ComposerIntent::Gateway,
        SlashCommand::Exit => ComposerIntent::Exit,
    }
}
