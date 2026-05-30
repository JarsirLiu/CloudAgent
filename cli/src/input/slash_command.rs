#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    Help,
    Copy,
    Interrupt,
    Compact,
    Session,
    NewConversation,
    SetTitle,
    ArchiveConversation,
    DeleteConversation,
    Filter,
    Permissions,
    Config,
    Skill,
    Gateway,
    Skills,
    Clear,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlashCommandSpec {
    pub(crate) command: SlashCommand,
    pub(crate) name: &'static str,
    pub(crate) aliases: &'static [&'static str],
    pub(crate) description: &'static str,
    pub(crate) argument_hint: Option<&'static str>,
    pub(crate) supports_inline_args: bool,
}

const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        command: SlashCommand::Help,
        name: "help",
        aliases: &[],
        description: "show available local commands",
        argument_hint: None,
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::Copy,
        name: "copy",
        aliases: &[],
        description: "copy the latest assistant reply",
        argument_hint: None,
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::Interrupt,
        name: "interrupt",
        aliases: &[],
        description: "interrupt the running turn",
        argument_hint: None,
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::Compact,
        name: "compact",
        aliases: &[],
        description: "compact older conversation context into a summary",
        argument_hint: None,
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::Session,
        name: "session",
        aliases: &[],
        description: "list sessions or switch with /session <id>",
        argument_hint: Some("<id>"),
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::NewConversation,
        name: "new",
        aliases: &[],
        description: "create and switch to a new session",
        argument_hint: Some("[session-id]"),
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::SetTitle,
        name: "title",
        aliases: &[],
        description: "set current session title",
        argument_hint: Some("<text>"),
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::ArchiveConversation,
        name: "archive",
        aliases: &[],
        description: "archive a conversation",
        argument_hint: Some("<id>"),
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::DeleteConversation,
        name: "delete",
        aliases: &[],
        description: "hard delete a conversation",
        argument_hint: Some("<id>"),
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::Filter,
        name: "filter",
        aliases: &[],
        description: "set pre-LLM input filter (use picker; state shown as filter on/off)",
        argument_hint: None,
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::Permissions,
        name: "permissions",
        aliases: &[],
        description: "set model execution permission mode",
        argument_hint: None,
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::Config,
        name: "config",
        aliases: &[],
        description: "open api config panel (api key / base url / model)",
        argument_hint: None,
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::Skill,
        name: "skill",
        aliases: &[],
        description: "insert a discovered skill into the composer as a structured skill item",
        argument_hint: Some("<name>"),
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::Gateway,
        name: "gateway",
        aliases: &[],
        description: "configure and connect IM platforms",
        argument_hint: None,
        supports_inline_args: true,
    },
    SlashCommandSpec {
        command: SlashCommand::Skills,
        name: "skills",
        aliases: &[],
        description: "list discovered skills",
        argument_hint: None,
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::Clear,
        name: "clear",
        aliases: &[],
        description: "clear this conversation",
        argument_hint: None,
        supports_inline_args: false,
    },
    SlashCommandSpec {
        command: SlashCommand::Exit,
        name: "exit",
        aliases: &[],
        description: "exit CloudAgent",
        argument_hint: None,
        supports_inline_args: false,
    },
];

impl SlashCommand {
    pub(crate) fn all() -> &'static [SlashCommandSpec] {
        SLASH_COMMANDS
    }

    pub(crate) fn spec(self) -> SlashCommandSpec {
        *SLASH_COMMANDS
            .iter()
            .find(|spec| spec.command == self)
            .expect("slash command spec must exist")
    }

    pub(crate) fn name(self) -> &'static str {
        self.spec().name
    }

    pub(crate) fn supports_inline_args(self) -> bool {
        self.spec().supports_inline_args
    }
}

pub(crate) fn find_slash_command(name: &str) -> Option<SlashCommand> {
    SLASH_COMMANDS
        .iter()
        .find(|spec| spec.matches_name(name))
        .map(|spec| spec.command)
}

pub(crate) fn slash_command_help_text() -> String {
    SLASH_COMMANDS
        .iter()
        .map(|spec| format!("/{:<12} {}", spec.name, spec.description))
        .collect::<Vec<_>>()
        .join("\n")
}

impl SlashCommandSpec {
    pub(crate) fn matches_name(self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    }

    pub(crate) fn matches_prefix(self, prefix: &str) -> bool {
        let prefix = prefix.to_ascii_lowercase();
        self.name.starts_with(&prefix)
            || self.aliases.iter().any(|alias| alias.starts_with(&prefix))
    }
}
