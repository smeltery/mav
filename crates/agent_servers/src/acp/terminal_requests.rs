use super::request_handlers::{respond_err, respond_result, session_thread};
use super::*;

pub(super) fn handle_create_terminal(
    args: acp::CreateTerminalRequest,
    responder: Responder<acp::CreateTerminalResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };
    let project = match thread
        .read_with(cx, |thread, _cx| thread.project().clone())
        .map_err(acp::Error::from)
    {
        Ok(p) => p,
        Err(e) => return respond_err(responder, e),
    };

    cx.spawn(async move |cx| {
        let result: Result<_, acp::Error> = async {
            let terminal_entity = acp_thread::create_terminal_entity(
                args.command.clone(),
                &args.args,
                args.env
                    .into_iter()
                    .map(|env| (env.name, env.value))
                    .collect(),
                args.cwd.clone(),
                &project,
                cx,
            )
            .await?;

            let terminal_entity = thread.update(cx, |thread, cx| {
                thread.register_terminal_created(
                    acp::TerminalId::new(uuid::Uuid::new_v4().to_string()),
                    format!("{} {}", args.command, args.args.join(" ")),
                    args.cwd.clone(),
                    args.output_byte_limit,
                    terminal_entity,
                    cx,
                )
            })?;
            let terminal_id = terminal_entity.read_with(cx, |terminal, _| terminal.id().clone());
            Ok(terminal_id)
        }
        .await;

        match result {
            Ok(terminal_id) => {
                responder
                    .respond(acp::CreateTerminalResponse::new(terminal_id))
                    .log_err();
            }
            Err(e) => respond_err(responder, e),
        }
    })
    .detach();
}

pub(super) fn handle_kill_terminal(
    args: acp::KillTerminalRequest,
    responder: Responder<acp::KillTerminalResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };

    match thread
        .update(cx, |thread, cx| thread.kill_terminal(args.terminal_id, cx))
        .flatten_acp()
    {
        Ok(()) => {
            responder
                .respond(acp::KillTerminalResponse::default())
                .log_err();
        }
        Err(e) => respond_err(responder, e),
    }
}

pub(super) fn handle_release_terminal(
    args: acp::ReleaseTerminalRequest,
    responder: Responder<acp::ReleaseTerminalResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };

    match thread
        .update(cx, |thread, cx| {
            thread.release_terminal(args.terminal_id, cx)
        })
        .flatten_acp()
    {
        Ok(()) => {
            responder
                .respond(acp::ReleaseTerminalResponse::default())
                .log_err();
        }
        Err(e) => respond_err(responder, e),
    }
}

pub(super) fn handle_terminal_output(
    args: acp::TerminalOutputRequest,
    responder: Responder<acp::TerminalOutputResponse>,
    cx: &mut AsyncApp,
    ctx: &ClientContext,
) {
    let thread = match session_thread(ctx, &args.session_id) {
        Ok(t) => t,
        Err(e) => return respond_err(responder, e),
    };

    match thread
        .read_with(cx, |thread, cx| -> anyhow::Result<_> {
            let out = thread
                .terminal(args.terminal_id)?
                .read(cx)
                .current_output(cx);
            Ok(out)
        })
        .flatten_acp()
    {
        Ok(output) => {
            responder.respond(output).log_err();
        }
        Err(e) => respond_err(responder, e),
    }
}

pub(super) fn handle_wait_for_terminal_exit(
    args: acp::WaitForTerminalExitRequest,
    responder: Responder<acp::WaitForTerminalExitResponse>,
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
                let exit_status = thread
                    .update(cx, |thread, cx| {
                        anyhow::Ok(thread.terminal(args.terminal_id)?.read(cx).wait_for_exit())
                    })
                    .flatten_acp()?
                    .await;
                Ok(exit_status)
            })
            .await;

        respond_result(responder, result.map(acp::WaitForTerminalExitResponse::new));
    })
    .detach();
}
