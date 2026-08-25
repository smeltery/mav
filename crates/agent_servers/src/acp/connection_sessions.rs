use super::*;

impl AcpConnection {
    pub fn prompt_capabilities(&self) -> &acp::PromptCapabilities {
        &self.agent_capabilities.prompt_capabilities
    }

    #[cfg(any(test, feature = "test-support"))]
    fn new_for_test(
        connection: ConnectionTo<Agent>,
        sessions: Rc<RefCell<HashMap<acp::SessionId, AcpSession>>>,
        agent_capabilities: acp::AgentCapabilities,
        agent_server_store: WeakEntity<AgentServerStore>,
        io_task: Task<()>,
        dispatch_task: Task<()>,
        cx: &mut App,
    ) -> Self {
        let agent_id = AgentId::new("test");
        let defaults = AcpConnectionDefaults::default();
        let settings_subscription = defaults.observe_settings(agent_id.clone(), cx);

        Self {
            id: agent_id,
            telemetry_id: "test".into(),
            agent_version: None,
            connection,
            sessions,
            pending_sessions: Rc::new(RefCell::new(HashMap::default())),
            auth_methods: vec![],
            agent_server_store,
            agent_capabilities,
            defaults,
            child: None,
            session_list: None,
            debug_log: AcpDebugLog::default(),
            _settings_subscription: settings_subscription,
            _io_task: io_task,
            _dispatch_task: dispatch_task,
            _wait_task: Task::ready(Ok(())),
            _stderr_task: Task::ready(Ok(())),
        }
    }

    pub(super) fn session_directories_from_work_dirs(
        &self,
        work_dirs: &PathList,
    ) -> Result<SessionDirectories> {
        let supports_additional_directories = self.supports_session_additional_directories();
        session_directories_from_work_dirs(work_dirs, supports_additional_directories)
    }

    pub(super) fn open_or_create_session(
        self: Rc<Self>,
        session_id: acp::SessionId,
        project: Entity<Project>,
        work_dirs: PathList,
        title: Option<SharedString>,
        rpc_call: impl FnOnce(
            ConnectionTo<Agent>,
            acp::SessionId,
            SessionDirectories,
        )
            -> futures::future::LocalBoxFuture<'static, Result<SessionConfigResponse>>
        + 'static,
        cx: &mut App,
    ) -> Task<Result<Entity<AcpThread>>> {
        // Check `pending_sessions` before `sessions` because the session is now
        // inserted into `sessions` before the load RPC completes (so that
        // notifications dispatched during history replay can find the thread).
        // Concurrent loads should still wait for the in-flight task so that
        // ref-counting happens in one place and the caller sees a fully loaded
        // session.
        if let Some(pending) = self.pending_sessions.borrow_mut().get_mut(&session_id) {
            pending.ref_count += 1;
            let task = pending.task.clone();
            return cx
                .foreground_executor()
                .spawn(async move { task.await.map_err(|err| anyhow!(err)) });
        }

        if let Some(session) = self.sessions.borrow_mut().get_mut(&session_id) {
            session.ref_count += 1;
            if let Some(thread) = session.thread.upgrade() {
                return Task::ready(Ok(thread));
            }
        }

        let directories = match self.session_directories_from_work_dirs(&work_dirs) {
            Ok(directories) => directories,
            Err(error) => return Task::ready(Err(error)),
        };

        let shared_task = cx
            .spawn({
                let session_id = session_id.clone();
                let this = self.clone();
                async move |cx| {
                    let action_log = cx.new(|_| ActionLog::new(project.clone()));
                    let thread: Entity<AcpThread> = cx.new(|cx| {
                        AcpThread::new(
                            None,
                            title,
                            Some(work_dirs),
                            this.clone(),
                            project,
                            action_log,
                            session_id.clone(),
                            watch::Receiver::constant(
                                this.agent_capabilities.prompt_capabilities.clone(),
                            ),
                            cx,
                        )
                    });

                    // Register the session before awaiting the RPC so that any
                    // `session/update` notifications that arrive during the call
                    // (e.g. history replay during `session/load`) can find the thread.
                    // Modes/config are filled in once the response arrives.
                    this.sessions.borrow_mut().insert(
                        session_id.clone(),
                        AcpSession {
                            thread: thread.downgrade(),
                            suppress_abort_err: false,
                            session_modes: None,
                            config_options: None,
                            ref_count: 1,
                        },
                    );

                    let response =
                        match rpc_call(this.connection.clone(), session_id.clone(), directories)
                            .await
                        {
                            Ok(response) => response,
                            Err(err) => {
                                this.sessions.borrow_mut().remove(&session_id);
                                this.pending_sessions.borrow_mut().remove(&session_id);
                                return Err(Arc::new(err));
                            }
                        };

                    let (modes, config_options) =
                        config_state(response.modes, response.config_options);

                    if let Some(config_opts) = config_options.as_ref() {
                        this.apply_default_config_options(&session_id, config_opts, cx);
                    }

                    let ref_count = this
                        .pending_sessions
                        .borrow_mut()
                        .remove(&session_id)
                        .map_or(1, |pending| pending.ref_count);

                    // If `close_session` ran to completion while the load RPC was in
                    // flight, it will have removed both the pending entry and the
                    // sessions entry (and dispatched the ACP close RPC). In that case
                    // the thread has no live session to attach to, so fail the load
                    // instead of handing back an orphaned thread.
                    {
                        let mut sessions = this.sessions.borrow_mut();
                        let Some(session) = sessions.get_mut(&session_id) else {
                            return Err(Arc::new(anyhow!(
                                "session was closed before load completed"
                            )));
                        };
                        session.session_modes = modes;
                        session.config_options = config_options.map(ConfigOptions::new);
                        session.ref_count = ref_count;
                    }

                    Ok(thread)
                }
            })
            .shared();

        self.pending_sessions.borrow_mut().insert(
            session_id,
            PendingAcpSession {
                task: shared_task.clone(),
                ref_count: 1,
            },
        );

        cx.foreground_executor()
            .spawn(async move { shared_task.await.map_err(|err| anyhow!(err)) })
    }

    pub(super) fn apply_default_config_options(
        &self,
        session_id: &acp::SessionId,
        config_options: &Rc<RefCell<Vec<acp::SessionConfigOption>>>,
        cx: &mut AsyncApp,
    ) {
        let id = self.id.clone();
        let apply_boolean_defaults = cx.update(|cx| cx.has_flag::<AcpBetaFeatureFlag>());
        let defaults_to_apply: Vec<_> = {
            let config_opts_ref = config_options.borrow();
            config_opts_ref
                .iter()
                .filter_map(|config_option| {
                    let default_value = self.defaults.config_option(config_option.id.0.as_ref())?;

                    let value_to_apply = match &config_option.kind {
                        acp::SessionConfigKind::Select(select) => {
                            let value_id = default_value.as_value_id()?;
                            match &select.options {
                                acp::SessionConfigSelectOptions::Ungrouped(options) => options
                                    .iter()
                                    .any(|opt| &*opt.value.0 == value_id)
                                    .then(|| {
                                        acp::SessionConfigOptionValue::value_id(
                                            value_id.to_string(),
                                        )
                                    }),
                                acp::SessionConfigSelectOptions::Grouped(groups) => groups
                                    .iter()
                                    .any(|group| {
                                        group.options.iter().any(|opt| &*opt.value.0 == value_id)
                                    })
                                    .then(|| {
                                        acp::SessionConfigOptionValue::value_id(
                                            value_id.to_string(),
                                        )
                                    }),
                                _ => None,
                            }
                        }
                        acp::SessionConfigKind::Boolean(_) if !apply_boolean_defaults => {
                            return None;
                        }
                        acp::SessionConfigKind::Boolean(_) => default_value
                            .as_bool()
                            .map(acp::SessionConfigOptionValue::boolean),
                        _ => None,
                    };

                    if let Some(value_to_apply) = value_to_apply {
                        let initial_value = match &config_option.kind {
                            acp::SessionConfigKind::Select(select) => {
                                acp::SessionConfigOptionValue::value_id(
                                    select.current_value.clone(),
                                )
                            }
                            acp::SessionConfigKind::Boolean(boolean) => {
                                acp::SessionConfigOptionValue::boolean(boolean.current_value)
                            }
                            _ => return None,
                        };

                        Some((config_option.id.clone(), value_to_apply, initial_value))
                    } else {
                        log::warn!(
                            "`{}` is not a valid value for config option `{}` in {}",
                            default_value,
                            config_option.id.0,
                            id
                        );
                        None
                    }
                })
                .collect()
        };

        for (config_id, default_value, initial_value) in defaults_to_apply {
            cx.spawn({
                let default_value_for_request = default_value.clone();
                let session_id = session_id.clone();
                let config_id_clone = config_id.clone();
                let config_opts = config_options.clone();
                let conn = self.connection.clone();
                async move |_| {
                    let result = conn
                        .send_request(acp::SetSessionConfigOptionRequest::new(
                            session_id,
                            config_id_clone.clone(),
                            default_value_for_request,
                        ))
                        .block_task()
                        .await
                        .log_err();

                    if result.is_none() {
                        let mut opts = config_opts.borrow_mut();
                        if let Some(opt) = opts.iter_mut().find(|o| o.id == config_id_clone) {
                            match (&mut opt.kind, &initial_value) {
                                (
                                    acp::SessionConfigKind::Select(select),
                                    acp::SessionConfigOptionValue::ValueId { value },
                                ) => {
                                    select.current_value = value.clone();
                                }
                                (
                                    acp::SessionConfigKind::Boolean(boolean),
                                    acp::SessionConfigOptionValue::Boolean { value },
                                ) => {
                                    boolean.current_value = *value;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            })
            .detach();

            let mut opts = config_options.borrow_mut();
            if let Some(opt) = opts.iter_mut().find(|o| o.id == config_id) {
                match (&mut opt.kind, &default_value) {
                    (
                        acp::SessionConfigKind::Select(select),
                        acp::SessionConfigOptionValue::ValueId { value },
                    ) => {
                        select.current_value = value.clone();
                    }
                    (
                        acp::SessionConfigKind::Boolean(boolean),
                        acp::SessionConfigOptionValue::Boolean { value },
                    ) => {
                        boolean.current_value = *value;
                    }
                    _ => {}
                }
            }
        }
    }
}
