use crate::app::conversation::actions::show_local_notice;
use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat};
use agent_app_server_client::AppServerClient;
use anyhow::Result;

pub(crate) async fn handle_skill_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalSkillInsert(name) => {
            let response = match client.request_skills_list_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Error,
                        format!("Failed to load skills: {err}"),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane
                .set_available_skills(response.skills.clone());
            let matches = response
                .skills
                .into_iter()
                .filter(|skill| skill.name.eq_ignore_ascii_case(&name))
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [] => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Warn,
                        format!(
                            "Skill '{name}' was not found. Use /skills to inspect available skills."
                        ),
                    );
                }
                [skill] => {
                    if !app
                        .bottom_pane
                        .attach_skill(skill.name.clone(), skill.path.display().to_string())
                    {
                        show_local_notice(
                            app,
                            crate::state::NoticeLevel::Warn,
                            "Close the active picker before inserting a skill.".to_string(),
                        );
                    } else {
                        show_local_notice(
                            app,
                            crate::state::NoticeLevel::Info,
                            format!(
                                "Inserted skill '{}'. Add your task text and submit when ready.",
                                skill.name
                            ),
                        );
                    }
                }
                _ => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Warn,
                        format!(
                            "Skill name '{name}' is ambiguous. Use /skills and pick a more specific name."
                        ),
                    );
                }
            }
            Ok(false)
        }
        ParsedInput::LocalSkillsOpen => {
            let response = match client.request_skills_list_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Error,
                        format!("Failed to load skills: {err}"),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane
                .set_available_skills(response.skills.clone());
            let text = if response.skills.is_empty() {
                "No skills discovered.\n\nChecked default locations:\n- <workspace>/.cloudagent/skills/\n- ~/.cloudagent/skills/".to_string()
            } else {
                let mut lines = Vec::new();
                lines.push("Discovered skills:".to_string());
                for skill in response.skills {
                    let mode = match skill.invocation_mode {
                        agent_core::SkillInvocationMode::Implicit => "implicit",
                        agent_core::SkillInvocationMode::Explicit => "explicit",
                    };
                    let deps = if skill.dependencies.tools.is_empty() {
                        String::new()
                    } else {
                        format!(" deps: {}", skill.dependencies.tools.join(", "))
                    };
                    lines.push(format!(
                        "- `{}` [{}]{}: {} ({})",
                        skill.name,
                        mode,
                        deps,
                        skill.description,
                        skill.path.display()
                    ));
                }
                lines.join("\n")
            };
            app.push_live_cell(HistoryCell::agent("skills", text, HistoryFormat::Markdown));
            Ok(false)
        }
        _ => unreachable!("skill input dispatcher received non-skill input"),
    }
}
