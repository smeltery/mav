use super::*;

pub(super) struct SessionDirectories {
    cwd: PathBuf,
    additional_directories: Vec<PathBuf>,
}

impl SessionDirectories {
    pub(super) fn into_new_session_request(self, mcp_servers: Vec<acp::McpServer>) -> acp::NewSessionRequest {
        acp::NewSessionRequest::new(self.cwd)
            .additional_directories(self.additional_directories)
            .mcp_servers(mcp_servers)
    }

    pub(super) fn into_load_session_request(
        self,
        session_id: acp::SessionId,
        mcp_servers: Vec<acp::McpServer>,
    ) -> acp::LoadSessionRequest {
        acp::LoadSessionRequest::new(session_id, self.cwd)
            .additional_directories(self.additional_directories)
            .mcp_servers(mcp_servers)
    }

    pub(super) fn into_resume_session_request(
        self,
        session_id: acp::SessionId,
        mcp_servers: Vec<acp::McpServer>,
    ) -> acp::ResumeSessionRequest {
        acp::ResumeSessionRequest::new(session_id, self.cwd)
            .additional_directories(self.additional_directories)
            .mcp_servers(mcp_servers)
    }
}

pub(super) fn session_directories_from_work_dirs(
    work_dirs: &PathList,
    supports_additional_directories: bool,
) -> Result<SessionDirectories> {
    let mut ordered_paths = work_dirs.ordered_paths();
    let cwd = ordered_paths
        .next()
        .cloned()
        .ok_or_else(|| anyhow!("Working directory cannot be empty"))?;
    let additional_directories = if supports_additional_directories {
        ordered_paths.cloned().collect()
    } else {
        Vec::new()
    };

    Ok(SessionDirectories {
        cwd,
        additional_directories,
    })
}

pub(super) fn work_dirs_from_session_info(
    cwd: PathBuf,
    additional_directories: Vec<PathBuf>,
) -> PathList {
    let mut seen_paths = HashSet::default();
    let mut paths = Vec::with_capacity(1 + additional_directories.len());

    seen_paths.insert(cwd.clone());
    paths.push(cwd);

    for path in additional_directories {
        if seen_paths.insert(path.clone()) {
            paths.push(path);
        }
    }

    PathList::new(&paths)
}

pub(super) fn emit_load_error_to_all_sessions(
    sessions: &Rc<RefCell<HashMap<acp::SessionId, AcpSession>>>,
    error: LoadError,
    cx: &mut AsyncApp,
) {
    let threads: Vec<_> = sessions
        .borrow()
        .values()
        .map(|session| session.thread.clone())
        .collect();

    for thread in threads {
        thread
            .update(cx, |thread, cx| thread.emit_load_error(error.clone(), cx))
            .ok();
    }
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            child.kill().log_err();
        }
    }
}

pub(super) fn terminal_auth_task_id(agent_id: &AgentId, method_id: &acp::AuthMethodId) -> String {
    format!("external-agent-{}-{}-login", agent_id.0, method_id.0)
}

pub(super) fn terminal_auth_task(
    command: &AgentServerCommand,
    agent_id: &AgentId,
    method: &acp::AuthMethodTerminal,
) -> SpawnInTerminal {
    acp_thread::build_terminal_auth_task(
        terminal_auth_task_id(agent_id, &method.id),
        method.name.clone(),
        command.path.to_string_lossy().into_owned(),
        command.args.clone(),
        command.env.clone().unwrap_or_default(),
    )
}

/// Used to support the _meta method prior to stabilization
pub(super) fn meta_terminal_auth_task(
    agent_id: &AgentId,
    method_id: &acp::AuthMethodId,
    method: &acp::AuthMethod,
) -> Option<SpawnInTerminal> {
    #[derive(Deserialize)]
    struct MetaTerminalAuth {
        label: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    }

    let meta = match method {
        acp::AuthMethod::EnvVar(env_var) => env_var.meta.as_ref(),
        acp::AuthMethod::Terminal(terminal) => terminal.meta.as_ref(),
        acp::AuthMethod::Agent(agent) => agent.meta.as_ref(),
        _ => None,
    }?;
    let terminal_auth =
        serde_json::from_value::<MetaTerminalAuth>(meta.get("terminal-auth")?.clone()).ok()?;

    Some(acp_thread::build_terminal_auth_task(
        terminal_auth_task_id(agent_id, method_id),
        terminal_auth.label.clone(),
        terminal_auth.command,
        terminal_auth.args,
        terminal_auth.env,
    ))
}
