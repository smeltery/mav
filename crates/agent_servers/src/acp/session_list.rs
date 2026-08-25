use super::session_helpers::work_dirs_from_session_info;
use super::*;

pub struct AcpSessionList {
    connection: ConnectionTo<Agent>,
    supports_delete: bool,
    updates_tx: async_channel::Sender<acp_thread::SessionListUpdate>,
    updates_rx: async_channel::Receiver<acp_thread::SessionListUpdate>,
}

impl AcpSessionList {
    pub(super) fn new(connection: ConnectionTo<Agent>, supports_delete: bool) -> Self {
        let (tx, rx) = async_channel::unbounded();
        Self {
            connection,
            supports_delete,
            updates_tx: tx,
            updates_rx: rx,
        }
    }

    fn notify_update(&self) {
        self.updates_tx
            .try_send(acp_thread::SessionListUpdate::Refresh)
            .log_err();
    }

    pub(super) fn send_info_update(
        &self,
        session_id: acp::SessionId,
        update: acp::SessionInfoUpdate,
    ) {
        self.updates_tx
            .try_send(acp_thread::SessionListUpdate::SessionInfo { session_id, update })
            .log_err();
    }
}

impl AgentSessionList for AcpSessionList {
    fn list_sessions(
        &self,
        request: AgentSessionListRequest,
        cx: &mut App,
    ) -> Task<Result<AgentSessionListResponse>> {
        let conn = self.connection.clone();
        cx.foreground_executor().spawn(async move {
            let acp_request = acp::ListSessionsRequest::new()
                .cwd(request.cwd)
                .cursor(request.cursor);
            let response = conn
                .send_request(acp_request)
                .block_task()
                .await
                .map_err(map_acp_error)?;
            Ok(AgentSessionListResponse {
                sessions: response
                    .sessions
                    .into_iter()
                    .map(|s| AgentSessionInfo {
                        session_id: s.session_id,
                        work_dirs: Some(work_dirs_from_session_info(
                            s.cwd,
                            s.additional_directories,
                        )),
                        title: s.title.map(Into::into),
                        updated_at: s.updated_at.and_then(|date_str| {
                            chrono::DateTime::parse_from_rfc3339(&date_str)
                                .ok()
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                        }),
                        created_at: None,
                        meta: s.meta,
                    })
                    .collect(),
                next_cursor: response.next_cursor,
                meta: response.meta,
            })
        })
    }

    fn supports_delete(&self) -> bool {
        self.supports_delete
    }

    fn delete_session(&self, session_id: &acp::SessionId, cx: &mut App) -> Task<Result<()>> {
        if !self.supports_delete() {
            return Task::ready(Err(anyhow::anyhow!("delete_session not supported")));
        }

        let conn = self.connection.clone();
        let updates_tx = self.updates_tx.clone();
        let session_id = session_id.clone();
        cx.foreground_executor().spawn(async move {
            conn.send_request(acp::DeleteSessionRequest::new(session_id))
                .block_task()
                .await
                .map_err(map_acp_error)?;
            updates_tx
                .try_send(acp_thread::SessionListUpdate::Refresh)
                .log_err();
            Ok(())
        })
    }

    fn watch(
        &self,
        _cx: &mut App,
    ) -> Option<async_channel::Receiver<acp_thread::SessionListUpdate>> {
        Some(self.updates_rx.clone())
    }

    fn notify_refresh(&self) {
        self.notify_update();
    }

    fn into_any(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }
}
