use super::client_transport::{client_capabilities_for_agent, connect_client_future};
use super::*;

impl AcpConnection {
    pub fn subscribe_debug_messages(
        &self,
    ) -> (
        Vec<AcpDebugMessage>,
        async_channel::Receiver<AcpDebugMessage>,
    ) {
        self.debug_log.subscribe()
    }

    pub async fn stdio(
        agent_id: AgentId,
        project: Entity<Project>,
        command: AgentServerCommand,
        agent_server_store: WeakEntity<AgentServerStore>,
        default_mode: Option<acp::SessionModeId>,
        default_config_options: HashMap<String, AgentConfigOptionValue>,
        cx: &mut AsyncApp,
    ) -> Result<Self> {
        let root_dir = project.read_with(cx, |project, cx| {
            project
                .default_path_list(cx)
                .ordered_paths()
                .next()
                .cloned()
        });
        let original_command = command.clone();
        let (path, args, env) = project
            .read_with(cx, |project, cx| {
                project.remote_client().and_then(|client| {
                    let template = client
                        .read(cx)
                        .build_command(
                            Some(command.path.display().to_string()),
                            &command.args,
                            &command.env.clone().into_iter().flatten().collect(),
                            root_dir.as_ref().map(|path| path.display().to_string()),
                            None,
                            Interactive::No,
                        )
                        .log_err()?;
                    Some((template.program, template.args, template.env))
                })
            })
            .unwrap_or_else(|| {
                (
                    command.path.display().to_string(),
                    command.args,
                    command.env.unwrap_or_default(),
                )
            });

        let builder = ShellBuilder::new(&Shell::System, cfg!(windows)).non_interactive();
        let mut child = builder.build_std_command(Some(path.clone()), &args);
        child.envs(env.clone());
        if let Some(cwd) = project.read_with(cx, |project, _cx| {
            if project.is_local() {
                root_dir.as_ref()
            } else {
                None
            }
        }) {
            child.current_dir(cwd);
        }
        let mut child = Child::spawn(child, Stdio::piped(), Stdio::piped(), Stdio::piped())?;

        let stdout = child.stdout.take().context("Failed to take stdout")?;
        let stdin = child.stdin.take().context("Failed to take stdin")?;
        let stderr = child.stderr.take().context("Failed to take stderr")?;
        log::debug!("Spawning external agent server: {:?}, {:?}", path, args);
        log::trace!("Spawned (pid: {})", child.id());

        let sessions = Rc::new(RefCell::new(HashMap::default()));
        let debug_log = AcpDebugLog::default();

        let (release_channel, version): (Option<&str>, String) = cx.update(|cx| {
            (
                release_channel::ReleaseChannel::try_global(cx)
                    .map(|release_channel| release_channel.display_name()),
                release_channel::AppVersion::global(cx).to_string(),
            )
        });

        let client_session_list: Rc<RefCell<Option<Rc<AcpSessionList>>>> =
            Rc::new(RefCell::new(None));

        // Set up the foreground dispatch channel for bridging Send handler
        // closures to the !Send foreground thread.
        let (dispatch_tx, dispatch_rx) = mpsc::unbounded::<ForegroundWork>();

        let incoming_lines = futures::io::BufReader::new(stdout).lines();
        let tapped_incoming = incoming_lines.inspect({
            let debug_log = debug_log.clone();
            move |result| match result {
                Ok(line) => debug_log.record_line(AcpDebugMessageDirection::Incoming, line),
                Err(err) => {
                    log::warn!("ACP transport read error: {err}");
                }
            }
        });

        let tapped_outgoing = futures::sink::unfold(
            (Box::pin(stdin), debug_log.clone()),
            async move |(mut writer, debug_log), line: String| {
                use futures::AsyncWriteExt;
                debug_log.record_line(AcpDebugMessageDirection::Outgoing, &line);
                let mut bytes = line.into_bytes();
                bytes.push(b'\n');
                writer.write_all(&bytes).await?;
                Ok::<_, std::io::Error>((writer, debug_log))
            },
        );

        let transport = Lines::new(tapped_outgoing, tapped_incoming);

        let stderr_task = cx.background_spawn({
            let debug_log = debug_log.clone();
            async move {
                let mut stderr = BufReader::new(stderr);
                let mut line = String::new();
                while let Ok(n) = stderr.read_line(&mut line).await
                    && n > 0
                {
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    log::warn!("agent stderr: {trimmed}");
                    debug_log.record_line(AcpDebugMessageDirection::Stderr, trimmed);
                    line.clear();
                }
                Ok(())
            }
        });

        // `connect_client_future` installs the production handler set and
        // hands us back both the connection-future (to run on a background
        // executor) and a oneshot receiver that produces the
        // `ConnectionTo<Agent>` once the transport handshake is ready.
        let (connection_tx, connection_rx) = futures::channel::oneshot::channel();
        let connection_future =
            connect_client_future("mav", transport, dispatch_tx.clone(), connection_tx);
        let io_task = cx.background_spawn(async move {
            if let Err(err) = connection_future.await {
                log::error!("ACP connection error: {err}");
            }
        });

        let connection_rx = async move {
            connection_rx
                .await
                .context("Failed to receive ACP connection handle")
        }
        .boxed_local();
        let status_fut = child
            .status()
            .map({
                let debug_log = debug_log.clone();
                move |status| match status {
                    Ok(status) => Ok(exited_load_error_with_stderr(status, &debug_log)),
                    Err(err) => Err(anyhow!("failed to wait for agent server exit: {err}")),
                }
            })
            .boxed_local();
        let (connection, status_fut) = match futures::future::select(connection_rx, status_fut)
            .await
        {
            futures::future::Either::Left((connection, status_fut)) => (connection?, status_fut),
            futures::future::Either::Right((load_error, _connection_rx)) => {
                return Err(load_error?.into());
            }
        };

        // Set up the foreground dispatch loop to process work items from handlers.
        let dispatch_context = ClientContext {
            sessions: sessions.clone(),
            session_list: client_session_list.clone(),
        };
        let dispatch_task = cx.spawn({
            let mut dispatch_rx = dispatch_rx;
            async move |cx| {
                while let Some(work) = dispatch_rx.next().await {
                    work.run(cx, &dispatch_context);
                }
            }
        });

        let initialize_response = connection
            .send_request(
                acp::InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(client_capabilities_for_agent(
                        &agent_id,
                        cx.update(|cx| cx.has_flag::<AcpBetaFeatureFlag>()),
                    ))
                    .client_info(
                        acp::Implementation::new("mav", version)
                            .title(release_channel.map(ToOwned::to_owned)),
                    ),
            )
            .block_task()
            .boxed_local();
        let (response, status_fut) =
            match futures::future::select(initialize_response, status_fut).await {
                futures::future::Either::Left((Ok(response), status_fut)) => (response, status_fut),
                futures::future::Either::Left((Err(error), status_fut)) => {
                    let timer = cx
                        .background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .boxed_local();
                    if let futures::future::Either::Left((load_error, _timer)) =
                        futures::future::select(status_fut, timer).await
                    {
                        return Err(load_error?.into());
                    }

                    return Err(error.into());
                }
                futures::future::Either::Right((load_error, _initialize_response)) => {
                    return Err(load_error?.into());
                }
            };

        if response.protocol_version < MINIMUM_SUPPORTED_VERSION {
            return Err(UnsupportedVersion.into());
        }

        let wait_task = cx.spawn({
            let sessions = sessions.clone();
            async move |cx| {
                let load_error = status_fut.await?;
                emit_load_error_to_all_sessions(&sessions, load_error, cx);
                anyhow::Ok(())
            }
        });

        let agent_info = response.agent_info;
        let telemetry_id = agent_info
            .as_ref()
            // Use the one the agent provides if we have one
            .map(|info| SharedString::from(info.name.clone()))
            // Otherwise, just use the name
            .unwrap_or_else(|| agent_id.0.clone());
        let agent_version = agent_info
            .and_then(|info| (!info.version.is_empty()).then(|| SharedString::from(info.version)));
        let agent_supports_delete = response
            .agent_capabilities
            .session_capabilities
            .delete
            .is_some();

        let session_list = if response
            .agent_capabilities
            .session_capabilities
            .list
            .is_some()
        {
            let list = Rc::new(AcpSessionList::new(
                connection.clone(),
                agent_supports_delete,
            ));
            *client_session_list.borrow_mut() = Some(list.clone());
            Some(list)
        } else {
            None
        };

        // TODO: Remove this override once Google team releases their official auth methods
        let auth_methods = if agent_id.0.as_ref() == GEMINI_ID {
            let mut gemini_args = original_command.args.clone();
            gemini_args.retain(|a| a != "--experimental-acp" && a != "--acp");
            let value = serde_json::json!({
                "label": "gemini /auth",
                "command": original_command.path.to_string_lossy(),
                "args": gemini_args,
                "env": original_command.env.unwrap_or_default(),
            });
            let meta = acp::Meta::from_iter([("terminal-auth".to_string(), value)]);
            vec![acp::AuthMethod::Agent(
                acp::AuthMethodAgent::new(GEMINI_TERMINAL_AUTH_METHOD_ID, "Login")
                    .description("Login with your Google or Vertex AI account")
                    .meta(meta),
            )]
        } else {
            response.auth_methods
        };
        let defaults = AcpConnectionDefaults::new(default_mode, default_config_options);
        let settings_subscription = cx.update({
            let agent_id = agent_id.clone();
            let defaults = defaults.clone();
            move |cx| defaults.observe_settings(agent_id, cx)
        });

        Ok(Self {
            id: agent_id,
            auth_methods,
            agent_server_store,
            connection,
            telemetry_id,
            agent_version,
            sessions,
            pending_sessions: Rc::new(RefCell::new(HashMap::default())),
            agent_capabilities: response.agent_capabilities,
            defaults,
            session_list,
            debug_log,
            _settings_subscription: settings_subscription,
            _io_task: io_task,
            _dispatch_task: dispatch_task,
            _wait_task: wait_task,
            _stderr_task: stderr_task,
            child: Some(child),
        })
    }
}
