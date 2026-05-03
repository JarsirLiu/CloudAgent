    use super::TuiApp;
    use crate::app::commands::parse::ParsedInput;
    use crate::app::conversation::actions::{execute_server_action, handle_tui_input};
    use crate::app::conversation::event_router;
    use crate::state::reducer::{ServerAction, TurnDispatch};
    use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
    use agent_app_server_client::{AppServerClient, AppServerEvent, InProcessClientConfig};
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, CommandExecutionStatus,
        ConversationStatus, ConversationTurn, ServerRequestDecisionKind, StructuredToolResult,
        FrontendMode, TranscriptItem, TurnItemKind,
    };
    use agent_runtime::AgentRuntime;
    use config::{AgentConfig, LlmConfig, RuntimeConfig, ToolConfig};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::OnceLock;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::time::timeout;

    fn flatten_turns(turns: Vec<ConversationTurn>) -> Vec<TranscriptItem> {
        turns
            .into_iter()
            .flat_map(|turn| turn.items.into_iter())
            .collect()
    }

mod ui_state;

    #[tokio::test]
    async fn end_to_end_turn_roundtrips_live_and_rebuilds_after_restart() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let expected_path = fixture.workspace.display().to_string();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_1",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"pwd\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![
                json!({
                    "model": "fake-model",
                    "choices": [{
                        "delta": {
                            "content": "current directory is "
                        }
                    }]
                }),
                json!({
                    "model": "fake-model",
                    "choices": [{
                        "delta": {
                            "content": expected_path
                        }
                    }]
                }),
            ]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));

        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime.clone(),
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );

        handle_tui_input(
            &mut app,
            &client,
            ParsedInput::Command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "可以看到当前在哪个目录下吗".to_string(),
                    turn_policy: agent_protocol::TurnPolicy {
                        permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                        approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                    },
                },
            )),
        )
        .expect("submit turn");

        let mut saw_server_request = false;
        let mut saw_turn_completed = false;
        while !saw_turn_completed {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            let request_id = match &event {
                AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) => Some(request_id.clone()),
                _ => None,
            };
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. }
                ))
            ) {
                saw_turn_completed = true;
            }
            event_router::handle_client_event(&mut app, event);
            if let Some(request_id) = request_id {
                saw_server_request = true;
                handle_tui_input(
                    &mut app,
                    &client,
                    ParsedInput::ServerRequestAnswer {
                        request_id,
                        decision: ServerRequestDecisionKind::Accept,
                        reason: "ok".to_string(),
                    },
                )
                .expect("approve request");
            }
        }
        assert!(
            !saw_server_request,
            "safe workspace read commands should not trigger approval"
        );

        let live_cells = app.transcript_state.transcript.cells();
        assert!(
            live_cells
                .iter()
                .any(|cell| cell.body == "可以看到当前在哪个目录下吗")
        );
        assert!(!live_cells.iter().any(|cell| cell.body == "approved"));
        assert!(live_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Control
                && cell.body.contains("inspect `pwd`")
                && cell.body.contains("exit 0")
        }));
        assert!(live_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
                && cell.body.starts_with("current directory is ")
                && cell.body.ends_with("\\workspace")
        }));

        client
            .send_command(AppClientCommand::RequestConversationStatus {
                conversation_id: "default".to_string(),
            })
            .expect("request status");
        client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history");

        let mut history = None;
        let mut status_idle = false;
        while history.is_none() || !status_idle {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for history")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => history = Some(flatten_turns(turns)),
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationStatus { snapshot, .. },
                )) => {
                    status_idle = matches!(snapshot.conversation_status, ConversationStatus::Idle)
                        && snapshot.active_turn.is_none();
                }
                other => event_router::handle_client_event(&mut app, other),
            }
        }
        client.shutdown().await.expect("shutdown client");

        let rollout_log = std::fs::read_to_string(fixture.store.join("default.rollout.jsonl"))
            .expect("read rollout log");
        assert!(
            rollout_log.contains("\"type\":\"event_msg\""),
            "rollout should persist EventMsg entries"
        );
        assert!(
            rollout_log.contains("\"type\":\"response_item\""),
            "rollout should persist ResponseItem entries"
        );

        let history = history.expect("history snapshot");
        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "可以看到当前在哪个目录下吗"
        )));
        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::CommandExecution {
                tool_name,
                command,
                ..
            } if tool_name == "shell_command" && command == "pwd"
        )));
        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::AgentMessage { text, .. }
            if text.starts_with("current directory is ") && text.ends_with("\\workspace")
        )));

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut restarted_app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );
        restarted_client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history after restart");

        let mut restarted_history_loaded = false;
        while !restarted_history_loaded {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting after restart")
                .expect("client event after restart");
            match &event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { .. },
                )) => restarted_history_loaded = true,
                _ => {}
            }
            event_router::handle_client_event(&mut restarted_app, event);
        }
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        let rebuilt_cells = restarted_app.transcript_state.transcript.cells();
        assert!(
            rebuilt_cells
                .iter()
                .any(|cell| cell.body == "可以看到当前在哪个目录下吗")
        );
        assert!(rebuilt_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Control
                && cell.body.contains("inspect `pwd`")
                && cell.body.contains("exit 0")
                && cell.body.ends_with("/workspace")
        }));
        assert!(rebuilt_cells.iter().any(|cell| {
            cell.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
                && cell.body.starts_with("current directory is ")
                && cell.body.ends_with("\\workspace")
        }));

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 2);
        assert!(recorded_requests[0].contains("\"stream\":true"));
        assert!(recorded_requests[1].contains("\"role\":\"tool\""));
        assert!(recorded_requests[1].contains("\"shell_command\""));
    }

    #[tokio::test]
    async fn interrupted_server_request_turn_rebuilds_tail_after_restart() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![sse_body(vec![json!({
            "model": "fake-model",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_interrupt",
                        "function": {
                            "name": "shell_command",
                            "arguments": "{\"command\":\"Set-Content out.txt hi\"}"
                        }
                    }]
                }
            }]
        })])];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );

        handle_tui_input(
            &mut app,
            &client,
            ParsedInput::Command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "帮我看看当前目录".to_string(),
                    turn_policy: agent_protocol::TurnPolicy {
                        permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                        approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                    },
                },
            )),
        )
        .expect("submit turn");

        let mut saw_server_request = false;
        let mut saw_server_request_cancelled = false;
        let mut saw_turn_cancelled = false;
        while !saw_turn_cancelled {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Request(_))
            ) {
                saw_server_request = true;
                let input = app
                    .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                    .expect("ctrl+c should produce interrupt input");
                handle_tui_input(&mut app, &client, input)
                    .expect("ctrl+c interrupt turn");
            }
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ServerRequestResolved {
                        decision,
                        ..
                    }
                )) if decision.decision == agent_protocol::ServerRequestDecisionKind::Cancel
            ) {
                saw_server_request_cancelled = true;
            }
            if matches!(
                &event,
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCancelled { .. }
                ))
            ) {
                saw_turn_cancelled = true;
            }
            event_router::handle_client_event(&mut app, event);
        }
        assert!(
            saw_server_request,
            "expected pending server request before interrupt"
        );
        assert!(
            saw_server_request_cancelled,
            "expected interrupt to cancel the pending server request"
        );

        client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history");

        let history = loop {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for history")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => break flatten_turns(turns),
                other => event_router::handle_client_event(&mut app, other),
            }
        };
        client.shutdown().await.expect("shutdown client");

        assert!(history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "帮我看看当前目录"
        )));

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut restarted_app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );
        restarted_client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history after restart");

        let mut restarted_history_loaded = false;
        while !restarted_history_loaded {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting after restart")
                .expect("client event after restart");
            match &event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { .. },
                )) => restarted_history_loaded = true,
                _ => {}
            }
            event_router::handle_client_event(&mut restarted_app, event);
        }
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        let rebuilt_cells = restarted_app.transcript_state.transcript.cells();
        let debug_cells = rebuilt_cells
            .iter()
            .map(|cell| (cell.label.as_str(), cell.body.as_str()))
            .collect::<Vec<_>>();
        assert!(
            rebuilt_cells
                .iter()
                .any(|cell| cell.body == "帮我看看当前目录")
        );
        assert_eq!(
            rebuilt_cells
                .iter()
                .filter(|cell| cell.body == "帮我看看当前目录")
                .count(),
            1
        );
        assert!(
            rebuilt_cells
                .iter()
                .any(|cell| cell.label == "shell_command"
                    && cell.body.contains("command `Set-Content out.txt hi`")),
            "rebuilt cells: {debug_cells:?}"
        );
        assert!(!rebuilt_cells.iter().any(|cell| cell.label == "request"));

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 1);
    }

    #[tokio::test]
    async fn consecutive_tool_turns_preserve_history_across_restart() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_one",
                            "function": {
                                "name": "shell_command",
                            "arguments": "{\"command\":\"pwd\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": "current directory is " } }]
                }),
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": fixture.workspace.display().to_string() } }]
                }),
            ]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_two",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"pwd\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": "again current directory is " } }]
                }),
                json!({
                    "model": "fake-model",
                    "choices": [{ "delta": { "content": fixture.workspace.display().to_string() } }]
                }),
            ]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );

        for content in ["第一轮看看目录", "第二轮再看一次目录"] {
            client
                .send_command(AppClientCommand::SubmitTurn(
                    agent_protocol::UserTurnInput {
                        conversation_id: "default".to_string(),
                        content: content.to_string(),
                        turn_policy: agent_protocol::TurnPolicy {
                            permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                            approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                        },
                    },
                ))
                .expect("submit turn");

            let mut saw_turn_completed = false;
            let mut saw_idle = false;
            while !saw_turn_completed || !saw_idle {
                let event = timeout(Duration::from_secs(10), client.next_event())
                    .await
                    .expect("timed out waiting for client event")
                    .expect("client event");
                if matches!(
                    &event,
                    AppServerEvent::Message(AppServerMessage::Notification(
                        AppServerNotification::TurnCompleted { .. }
                    ))
                ) {
                    saw_turn_completed = true;
                }
                if matches!(
                    &event,
                    AppServerEvent::Message(AppServerMessage::Notification(
                        AppServerNotification::FrontendStateChanged {
                            mode: agent_protocol::FrontendMode::Idle,
                            ..
                        }
                    ))
                ) {
                    saw_idle = true;
                }
                event_router::handle_client_event(&mut app, event);
            }
        }

        client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request live history");
        let live_history = loop {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for history")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => break flatten_turns(turns),
                other => event_router::handle_client_event(&mut app, other),
            }
        };
        client.shutdown().await.expect("shutdown client");

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        restarted_client
            .send_command(AppClientCommand::RequestConversationHistory {
                conversation_id: "default".to_string(),
            })
            .expect("request history after restart");
        let restarted_history = loop {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting after restart")
                .expect("client event after restart");
            match event {
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::ConversationHistory { turns, .. },
                )) => break flatten_turns(turns),
                _ => {}
            }
        };
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        assert_eq!(restarted_history.len(), live_history.len());
        assert!(restarted_history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "第一轮看看目录"
        )));
        assert!(restarted_history.iter().any(|entry| matches!(
            entry,
            TranscriptItem::UserMessage { text, .. } if text == "第二轮再看一次目录"
        )));
        assert!(restarted_history.iter().filter(|entry| matches!(
            entry,
            TranscriptItem::AgentMessage { text, .. } if text.starts_with("current directory is ")
        )).count() >= 1);
        assert!(restarted_history.iter().filter(|entry| matches!(
            entry,
            TranscriptItem::AgentMessage { text, .. } if text.starts_with("again current directory is ")
        )).count() >= 1);

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 4);
    }

    #[tokio::test]
    async fn restarted_turn_uses_rollout_history_in_model_request() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "content": "first answer"
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "content": "second answer"
                    }
                }]
            })]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));

        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime.clone(),
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );

        handle_tui_input(
            &mut app,
            &client,
            ParsedInput::Command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "first question".to_string(),
                    turn_policy: agent_protocol::TurnPolicy {
                        permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                        approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                    },
                },
            )),
        )
        .expect("submit first turn");

        while !matches!(app.console_state.mode, FrontendMode::Idle) {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for first turn")
                .expect("client event");
            event_router::handle_client_event(&mut app, event);
        }
        client.shutdown().await.expect("shutdown first client");

        let runtime_after_restart =
            Arc::new(AgentRuntime::from_config((*config).clone()).expect("restart runtime"));
        let mut restarted_client = AppServerClient::in_process(InProcessClientConfig {
            runtime: runtime_after_restart,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut restarted_app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );

        handle_tui_input(
            &mut restarted_app,
            &restarted_client,
            ParsedInput::Command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "second question".to_string(),
                    turn_policy: agent_protocol::TurnPolicy {
                        permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                        approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                    },
                },
            )),
        )
        .expect("submit second turn");

        while !matches!(restarted_app.console_state.mode, FrontendMode::Idle) {
            let event = timeout(Duration::from_secs(10), restarted_client.next_event())
                .await
                .expect("timed out waiting for restarted turn")
                .expect("client event after restart");
            event_router::handle_client_event(&mut restarted_app, event);
        }
        restarted_client
            .shutdown()
            .await
            .expect("shutdown restarted client");

        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 2);
        assert!(recorded_requests[1].contains("first question"));
        assert!(recorded_requests[1].contains("first answer"));
        assert!(recorded_requests[1].contains("second question"));
    }

    #[tokio::test]
    async fn cli_settings_persist_in_sqlite_and_reload() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            "http://127.0.0.1:9".to_string(),
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });
        let mut app = TuiApp::new(
            "default".to_string(),
            "in-process",
            fixture.workspace.clone(),
            fixture.store.clone(),
            false,
            "safe".to_string(),
        );

        handle_tui_input(
            &mut app,
            &client,
            ParsedInput::LocalFilterToggle("on".to_string()),
        )
        .expect("toggle filter");
        handle_tui_input(
            &mut app,
            &client,
            ParsedInput::LocalPermissionMode("danger".to_string()),
        )
        .expect("set permission mode");

        let settings = crate::app::cli_settings::load_cli_settings(&fixture.store)
            .expect("load cli settings")
            .expect("persisted settings");
        assert!(settings.pre_llm_filter_enabled);
        assert_eq!(settings.permission_mode, "danger");
    }

    #[tokio::test]
    async fn denied_tool_in_multi_tool_batch_still_records_all_tool_results() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [
                            {
                                "index": 0,
                            "id": "call_denied",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"Set-Content out.txt hi\"}"
                            }
                        },
                        {
                            "index": 1,
                            "id": "call_allowed",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"Set-Content other.txt hi\"}"
                            }
                        }
                        ]
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{ "delta": { "content": "done" } }]
            })]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });

        client
            .send_command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "run two commands".to_string(),
                    turn_policy: agent_protocol::TurnPolicy {
                        permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                        approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                    },
                },
            ))
            .expect("submit turn");

        let mut request_count = 0usize;
        let mut saw_completed = false;
        while !saw_completed {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) => {
                    request_count += 1;
                    let decision = if request_count == 1 {
                        agent_protocol::ServerRequestDecision::decline(Some(
                            "skip first".to_string(),
                        ))
                    } else {
                        agent_protocol::ServerRequestDecision::accept(Some("ok".to_string()))
                    };
                    client
                        .send_command(AppClientCommand::ResolveServerRequest {
                            conversation_id: "default".to_string(),
                            request_id,
                            decision,
                        })
                        .expect("resolve request");
                }
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. },
                )) => {
                    saw_completed = true;
                }
                _ => {}
            }
        }
        client.shutdown().await.expect("shutdown client");

        assert_eq!(request_count, 2);
        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 2);
        assert!(recorded_requests[1].contains("\"tool_call_id\":\"call_denied\""));
        assert!(recorded_requests[1].contains("\"tool_call_id\":\"call_allowed\""));
    }

    #[tokio::test]
    async fn repeated_denied_tool_request_does_not_prompt_again() {
        let _guard = cli_e2e_test_lock().await;
        let fixture = TempFixture::new();
        let responses = vec![
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_denied_once",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"df -h\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_denied_repeat",
                            "function": {
                                "name": "shell_command",
                                "arguments": "{\"command\":\"df -h\"}"
                            }
                        }]
                    }
                }]
            })]),
            sse_body(vec![json!({
                "model": "fake-model",
                "choices": [{ "delta": { "content": "I cannot inspect disk usage because permission was denied." } }]
            })]),
        ];
        let (base_url, server_thread) = spawn_fake_llm_server(responses);
        let config = Arc::new(test_config(
            fixture.workspace.clone(),
            fixture.store.clone(),
            base_url,
        ));
        let runtime = Arc::new(AgentRuntime::from_config((*config).clone()).expect("runtime"));
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime,
            conversation_id: "default".to_string(),
            auto_approve: false,
            auto_approve_reason: None,
        });

        client
            .send_command(AppClientCommand::SubmitTurn(
                agent_protocol::UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: "check disk".to_string(),
                    turn_policy: agent_protocol::TurnPolicy {
                        permission_profile: agent_protocol::PermissionProfile::ReadOnly,
                        approval_policy: agent_protocol::ApprovalPolicy::OnRequest,
                    },
                },
            ))
            .expect("submit turn");

        let mut request_count = 0usize;
        let mut saw_completed = false;
        while !saw_completed {
            let event = timeout(Duration::from_secs(10), client.next_event())
                .await
                .expect("timed out waiting for client event")
                .expect("client event");
            match event {
                AppServerEvent::Message(AppServerMessage::Request(
                    agent_protocol::AppServerRequest::ServerRequest { request_id, .. },
                )) => {
                    request_count += 1;
                    client
                        .send_command(AppClientCommand::ResolveServerRequest {
                            conversation_id: "default".to_string(),
                            request_id,
                            decision: agent_protocol::ServerRequestDecision::decline(Some(
                                String::new(),
                            )),
                        })
                        .expect("deny request");
                }
                AppServerEvent::Message(AppServerMessage::Notification(
                    AppServerNotification::TurnCompleted { .. },
                )) => {
                    saw_completed = true;
                }
                _ => {}
            }
        }
        client.shutdown().await.expect("shutdown client");

        assert_eq!(request_count, 1);
        let recorded_requests = server_thread
            .join()
            .expect("fake llm server thread panicked")
            .expect("fake llm server");
        assert_eq!(recorded_requests.len(), 3);
        assert!(recorded_requests[1].contains("\"tool_call_id\":\"call_denied_once\""));
        assert!(recorded_requests[1].contains("exec command rejected by user"));
        assert!(recorded_requests[2].contains("\"tool_call_id\":\"call_denied_repeat\""));
        assert!(recorded_requests[2].contains("exec command rejected by user"));
        assert!(recorded_requests[2].contains("same tool request was already denied in this turn"));
    }

    fn test_config(
        workspace_root: PathBuf,
        conversation_store_dir: PathBuf,
        base_url: String,
    ) -> AgentConfig {
        AgentConfig {
            workspace_root,
            llm: LlmConfig {
                base_url,
                api_key: "test-key".to_string(),
                model: "fake-model".to_string(),
                temperature: 0.0,
            },
            runtime: RuntimeConfig {
                system_prompt: "You are a test agent.".to_string(),
                max_tool_roundtrips: Some(4),
                conversation_store_dir,
                model_context_window: 200_000,
                context_compaction_trigger_ratio: 0.90,
                context_compaction_target_tokens: 36_000,
                context_compaction_request_overhead_tokens: 28_000,
                context_compaction_preserved_user_turns: 3,
                context_compaction_preserved_tail_tokens: 12_000,
                context_compaction_summary_source_tokens: 24_000,
                memory: Default::default(),
                enable_skill_bucket: false,
                enable_mcp_bucket: false,
                post_compact_token_budget: 50_000,
                post_compact_memory_floor_tokens: 6_000,
                post_compact_skills_token_budget: 25_000,
                post_compact_mcp_token_budget: 8_000,
                post_compact_max_tokens_per_memory: 6_000,
                post_compact_max_tokens_per_skill: 5_000,
                post_compact_max_tokens_per_mcp: 3_000,
                context_budget_safety_buffer_tokens: 8_000,
            },
            tools: ToolConfig {
                default_shell_timeout_ms: 5_000,
                max_read_chars: 8_192,
            },
            cli: config::CliConfig {
                pre_llm_filter_enabled: false,
                permission_mode: "safe".to_string(),
            },
        }
    }

    fn sse_body(chunks: Vec<serde_json::Value>) -> String {
        let mut body = String::new();
        for chunk in chunks {
            body.push_str("data: ");
            body.push_str(&serde_json::to_string(&chunk).expect("sse chunk"));
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        body
    }

    fn spawn_fake_llm_server(
        responses: Vec<String>,
    ) -> (String, thread::JoinHandle<std::io::Result<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake llm server");
        let base_url = format!("http://{}", listener.local_addr().expect("listener addr"));
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let request_body = read_http_request_body(&mut stream)?;
                requests.push(request_body);
                let http_response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    response.len(),
                    response
                );
                stream.write_all(http_response.as_bytes())?;
                stream.flush()?;
            }
            Ok(requests)
        });
        (base_url, handle)
    }

    fn read_http_request_body(stream: &mut TcpStream) -> std::io::Result<String> {
        let mut buffer = Vec::new();
        let mut scratch = [0u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut scratch)?;
            if read == 0 {
                return Ok(String::new());
            }
            buffer.extend_from_slice(&scratch[..read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut scratch)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&scratch[..read]);
        }
        body.truncate(content_length);
        Ok(String::from_utf8_lossy(&body).to_string())
    }

    struct TempFixture {
        root: PathBuf,
        workspace: PathBuf,
        store: PathBuf,
    }

    impl TempFixture {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("cloudagent-cli-test-{unique}"));
            let workspace = root.join("workspace");
            let store = root.join("conversations");
            std::fs::create_dir_all(&workspace).expect("create workspace");
            std::fs::create_dir_all(&store).expect("create conversation store");
            Self {
                root,
                workspace,
                store,
            }
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn cli_e2e_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }
