use super::*;

pub(super) fn session_thread(
    ctx: &ClientContext,
    session_id: &acp::SessionId,
) -> Result<WeakEntity<AcpThread>, acp::Error> {
    let sessions = ctx.sessions.borrow();
    sessions
        .get(session_id)
        .map(|session| session.thread.clone())
        .ok_or_else(|| acp::Error::internal_error().data(format!("unknown session: {session_id}")))
}

pub(super) fn respond_err<T: JsonRpcResponse>(responder: Responder<T>, err: acp::Error) {
    // Log the actual error we're returning — otherwise agents that hit an
    // error path (e.g. unknown session) would see only the generic internal
    // error returned over the wire with no trace of why on the client side.
    log::warn!(
        "Responding to ACP request `{method}` with error: {err:?}",
        method = responder.method()
    );
    responder.respond_with_error(err).log_err();
}

pub(super) fn respond_result<T: JsonRpcResponse>(
    responder: Responder<T>,
    result: Result<T, acp::Error>,
) {
    match result {
        Ok(response) => {
            responder.respond(response).log_err();
        }
        Err(err) => respond_err(responder, err),
    }
}

pub(super) fn handle_request_permission(
    args: acp::RequestPermissionRequest,
    responder: Responder<acp::RequestPermissionResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };

    let cancellation = responder.cancellation();
    let tool_call_id = args.tool_call.tool_call_id.clone();
    cx.spawn(async move |cx| {
        let result: Result<_, acp::Error> = async {
            let task = thread
                .update(cx, |thread, cx| {
                    thread.request_tool_call_authorization(
                        args.tool_call,
                        acp_thread::PermissionOptions::Flat(args.options),
                        acp_thread::AuthorizationKind::PermissionGrant,
                        cx,
                    )
                })
                .flatten_acp()?;
            cancellation
                .run_until_cancelled(async { Ok(task.await) })
                .await
        }
        .await;

        match result {
            Ok(outcome) => {
                responder
                    .respond(acp::RequestPermissionResponse::new(outcome.into()))
                    .log_err();
            }
            Err(e) => {
                if e.code == ErrorCode::RequestCancelled {
                    thread
                        .update(cx, |thread, cx| {
                            thread.cancel_tool_call_authorization(&tool_call_id, cx)
                        })
                        .log_err();
                }
                respond_err(responder, e)
            }
        }
    })
    .detach();
}

pub(super) fn handle_write_text_file(
    args: acp::WriteTextFileRequest,
    responder: Responder<acp::WriteTextFileResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };

    cx.spawn(async move |cx| {
        let result: Result<_, acp::Error> = async {
            thread
                .update(cx, |thread, cx| {
                    thread.write_text_file(args.path, args.content, cx)
                })
                .map_err(acp::Error::from)?
                .await?;
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                responder
                    .respond(acp::WriteTextFileResponse::default())
                    .log_err();
            }
            Err(e) => respond_err(responder, e),
        }
    })
    .detach();
}

pub(super) fn handle_read_text_file(
    args: acp::ReadTextFileRequest,
    responder: Responder<acp::ReadTextFileResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };

    cx.spawn(async move |cx| {
        let cancellation = responder.cancellation();
        let result = cancellation
            .run_until_cancelled(async {
                thread
                    .update(cx, |thread, cx| {
                        thread.read_text_file(args.path, args.line, args.limit, false, cx)
                    })
                    .map_err(acp::Error::from)?
                    .await
            })
            .await;

        respond_result(responder, result.map(acp::ReadTextFileResponse::new));
    })
    .detach();
}

pub(super) fn handle_session_notification(
    notification: acp::SessionNotification,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    // Extract everything we need from the session while briefly borrowing.
    let (thread, session_modes, config_opts_data) = {
        let sessions = ctx.sessions.borrow();
        let Some(session) = sessions.get(&notification.session_id) else {
            log::warn!(
                "Received session notification for unknown session: {:?}",
                notification.session_id
            );
            return;
        };
        (
            session.thread.clone(),
            session.session_modes.clone(),
            session
                .config_options
                .as_ref()
                .map(|opts| (opts.config_options.clone(), opts.tx.clone())),
        )
    };
    // Borrow is dropped here.

    // Apply mode/config/session_list updates without holding the borrow.
    if let acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate {
        current_mode_id, ..
    }) = &notification.update
    {
        if let Some(session_modes) = &session_modes {
            session_modes.borrow_mut().current_mode_id = current_mode_id.clone();
        }
    }

    if let acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate {
        config_options, ..
    }) = &notification.update
    {
        if let Some((config_opts_cell, tx_cell)) = &config_opts_data {
            *config_opts_cell.borrow_mut() = config_options.clone();
            tx_cell.borrow_mut().send(()).ok();
        }
    }

    if let acp::SessionUpdate::SessionInfoUpdate(info_update) = &notification.update
        && let Some(session_list) = ctx.session_list.borrow().as_ref()
    {
        session_list.send_info_update(notification.session_id.clone(), info_update.clone());
    }

    // Pre-handle: if a ToolCall carries terminal_info, create/register a display-only terminal.
    if let acp::SessionUpdate::ToolCall(tc) = &notification.update {
        if let Some(meta) = &tc.meta {
            if let Some(terminal_info) = meta.get("terminal_info") {
                if let Some(id_str) = terminal_info.get("terminal_id").and_then(|v| v.as_str()) {
                    let terminal_id = acp::TerminalId::new(id_str);
                    let cwd = terminal_info
                        .get("cwd")
                        .and_then(|v| v.as_str().map(PathBuf::from));

                    thread
                        .update(cx, |thread, cx| {
                            let builder = TerminalBuilder::new_display_only(
                                CursorShape::default(),
                                AlternateScroll::On,
                                None,
                                0,
                                cx.background_executor(),
                                thread.project().read(cx).path_style(cx),
                            );
                            let lower = cx.new(|cx| builder.subscribe(cx));
                            thread.on_terminal_provider_event(
                                TerminalProviderEvent::Created {
                                    terminal_id,
                                    label: tc.title.clone(),
                                    cwd,
                                    output_byte_limit: None,
                                    terminal: lower,
                                },
                                cx,
                            );
                        })
                        .log_err();
                }
            }
        }
    }

    // Forward the update to the acp_thread as usual.
    if let Err(err) = thread
        .update(cx, |thread, cx| {
            thread.handle_session_update(notification.update.clone(), cx)
        })
        .flatten_acp()
    {
        log::error!(
            "Failed to handle session update for {:?}: {err:?}",
            notification.session_id
        );
    }

    // Post-handle: stream terminal output/exit if present on ToolCallUpdate meta.
    if let acp::SessionUpdate::ToolCallUpdate(tcu) = &notification.update {
        if let Some(meta) = &tcu.meta {
            if let Some(term_out) = meta.get("terminal_output") {
                if let Some(id_str) = term_out.get("terminal_id").and_then(|v| v.as_str()) {
                    let terminal_id = acp::TerminalId::new(id_str);
                    if let Some(s) = term_out.get("data").and_then(|v| v.as_str()) {
                        let data = s.as_bytes().to_vec();
                        thread
                            .update(cx, |thread, cx| {
                                thread.on_terminal_provider_event(
                                    TerminalProviderEvent::Output { terminal_id, data },
                                    cx,
                                );
                            })
                            .log_err();
                    }
                }
            }

            if let Some(term_exit) = meta.get("terminal_exit") {
                if let Some(id_str) = term_exit.get("terminal_id").and_then(|v| v.as_str()) {
                    let terminal_id = acp::TerminalId::new(id_str);
                    let status = acp::TerminalExitStatus::new()
                        .exit_code(
                            term_exit
                                .get("exit_code")
                                .and_then(|v| v.as_u64())
                                .map(|i| i as u32),
                        )
                        .signal(
                            term_exit
                                .get("signal")
                                .and_then(|v| v.as_str().map(|s| s.to_string())),
                        );

                    thread
                        .update(cx, |thread, cx| {
                            thread.on_terminal_provider_event(
                                TerminalProviderEvent::Exit {
                                    terminal_id,
                                    status,
                                },
                                cx,
                            );
                        })
                        .log_err();
                }
            }
        }
    }
}
