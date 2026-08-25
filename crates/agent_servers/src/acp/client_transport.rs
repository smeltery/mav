use super::request_handlers::{
    handle_read_text_file, handle_request_permission, handle_session_notification,
    handle_write_text_file,
};
use super::*;

pub(super) fn connect_client_future(
    name: &'static str,
    transport: impl agent_client_protocol::ConnectTo<Client> + 'static,
    dispatch_tx: mpsc::UnboundedSender<ForegroundWork>,
    connection_tx: futures::channel::oneshot::Sender<ConnectionTo<Agent>>,
) -> impl Future<Output = Result<(), acp::Error>> {
    // Each handler forwards its inputs onto the foreground dispatch queue.
    // The SDK requires the closure to be `Send`, so we move a clone of
    // `dispatch_tx` into each one.
    macro_rules! on_request {
        ($handler:ident) => {{
            let dispatch_tx = dispatch_tx.clone();
            async move |req, responder, _connection| {
                enqueue_request(&dispatch_tx, req, responder, $handler);
                Ok(())
            }
        }};
    }
    macro_rules! on_notification {
        ($handler:ident) => {{
            let dispatch_tx = dispatch_tx.clone();
            async move |notif, connection| {
                enqueue_notification(&dispatch_tx, notif, connection, $handler);
                Ok(())
            }
        }};
    }

    Client
        .builder()
        .name(name)
        // --- Request handlers (agent→client) ---
        .on_receive_request(
            on_request!(handle_request_permission),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_write_text_file),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_read_text_file),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_create_terminal),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_kill_terminal),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_release_terminal),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_terminal_output),
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            on_request!(handle_wait_for_terminal_exit),
            agent_client_protocol::on_receive_request!(),
        )
        // --- Notification handlers (agent→client) ---
        .on_receive_notification(
            on_notification!(handle_session_notification),
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(
            transport,
            move |connection: ConnectionTo<Agent>| async move {
                if connection_tx.send(connection).is_err() {
                    log::error!("failed to send ACP connection handle — receiver was dropped");
                }
                // Keep the connection alive until the transport closes.
                futures::future::pending::<Result<(), acp::Error>>().await
            },
        )
}

pub(super) fn client_capabilities_for_agent(
    agent_id: &AgentId,
    supports_boolean_config_options: bool,
) -> acp::ClientCapabilities {
    let mut meta = acp::Meta::from_iter([
        ("terminal_output".into(), true.into()),
        ("terminal-auth".into(), true.into()),
    ]);

    if agent_id.as_ref() == CURSOR_ID {
        meta.insert(PARAMETERIMAV_MODEL_PICKER_META_KEY.into(), true.into());
    }

    let mut capabilities = acp::ClientCapabilities::new()
        .fs(acp::FileSystemCapabilities::new()
            .read_text_file(true)
            .write_text_file(true))
        .terminal(true)
        .auth(acp::AuthCapabilities::new().terminal(true))
        .meta(meta);

    if supports_boolean_config_options {
        capabilities = capabilities.session(
            acp::ClientSessionCapabilities::new().config_options(
                acp::SessionConfigOptionsCapabilities::new()
                    .boolean(acp::BooleanConfigOptionCapabilities::new()),
            ),
        );
    }

    capabilities
}
