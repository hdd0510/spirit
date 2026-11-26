use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use instant::Instant;
use warpui::{AppContext, Entity, EntityId, ModelContext, ModelHandle, SingletonEntity};

use super::core::subscribe_to_shared_dependencies;
use super::{
    InlineItem, SlashCommandDataSource, SlashCommandDataSourceState, UpdatedActiveCommands,
};
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::search::slash_command_menu::StaticCommand;
use crate::search::slash_command_menu::static_commands::Availability;
use crate::search::slash_command_menu::static_commands::commands::COMMAND_REGISTRY;
use crate::settings::{InputSettings, InputSettingsChangedEvent};
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::{CLIAgentSessionsModel, CLIAgentSessionsModelEvent};
use crate::terminal::input::slash_commands::AcceptSlashCommandOrSavedPrompt;
#[cfg(all(not(test), feature = "local_fs"))]
use crate::terminal::input::slash_commands::agent_commands::probe_agent_commands;
use crate::terminal::input::slash_commands::agent_commands::{
    AgentCommand, fallback_commands, supports_agent_commands,
};
#[cfg(all(not(test), feature = "local_fs"))]
use crate::terminal::local_shell::LocalShellState;
#[cfg(all(not(test), feature = "local_fs"))]
use crate::terminal::model::session::SessionType;
use crate::terminal::model::session::active_session::ActiveSession;

const AGENT_COMMANDS_TTL: Duration = Duration::from_secs(30);

pub struct GuiDataSourceArgs {
    pub active_session: ModelHandle<ActiveSession>,
    pub terminal_view_id: EntityId,
}

pub struct GuiSlashCommandDataSource {
    state: SlashCommandDataSourceState,
    agent: Option<CLIAgent>,
    agent_commands_requested_at: Option<Instant>,
}

impl GuiSlashCommandDataSource {
    pub fn new(args: GuiDataSourceArgs, ctx: &mut ModelContext<Self>) -> Self {
        let GuiDataSourceArgs {
            active_session,
            terminal_view_id,
        } = args;

        subscribe_to_shared_dependencies(
            &active_session,
            terminal_view_id,
            Self::recompute_active_commands,
            ctx,
        );
        ctx.subscribe_to_model(
            &CLIAgentSessionsModel::handle(ctx),
            move |me, _, event, ctx| {
                if event.terminal_view_id() == terminal_view_id
                    && matches!(
                        event,
                        CLIAgentSessionsModelEvent::InputSessionChanged { .. }
                    )
                {
                    me.refresh_agent_commands(ctx);
                }
            },
        );
        ctx.subscribe_to_model(&InputSettings::handle(ctx), |me, _, event, ctx| {
            if matches!(
                event,
                InputSettingsChangedEvent::EnableSlashCommandsInTerminal { .. }
            ) {
                me.recompute_active_commands(ctx);
            }
        });

        let mut me = Self {
            state: SlashCommandDataSourceState::new(active_session, terminal_view_id),
            agent: None,
            agent_commands_requested_at: None,
        };
        me.recompute_active_commands(ctx);
        me
    }

    pub fn refresh_agent_commands(&mut self, ctx: &mut ModelContext<Self>) {
        let sessions = CLIAgentSessionsModel::as_ref(ctx);
        let terminal_view_id = self.terminal_view_id();
        let agent = sessions
            .session(terminal_view_id)
            .filter(|_| sessions.is_input_open(terminal_view_id))
            .map(|session| session.agent)
            .filter(|agent| supports_agent_commands(*agent));
        let Some(agent) = agent else {
            return;
        };
        if self.agent != Some(agent) {
            self.agent = Some(agent);
            self.agent_commands_requested_at = None;
            self.set_agent_commands(fallback_commands(agent), ctx);
        }
        if self
            .agent_commands_requested_at
            .is_some_and(|at| at.elapsed() < AGENT_COMMANDS_TTL)
        {
            return;
        }
        self.agent_commands_requested_at = Some(Instant::now());
        self.request_agent_commands(agent, ctx);
    }

    #[cfg(any(test, not(feature = "local_fs")))]
    fn request_agent_commands(&mut self, _agent: CLIAgent, _ctx: &mut ModelContext<Self>) {}

    #[cfg(all(not(test), feature = "local_fs"))]
    fn request_agent_commands(&mut self, agent: CLIAgent, ctx: &mut ModelContext<Self>) {
        let session = self.active_session().as_ref(ctx);
        if session.session_type(ctx) != Some(SessionType::Local) {
            return;
        }
        let Some(cwd) = session.current_working_directory().map(PathBuf::from) else {
            return;
        };
        let path_env = LocalShellState::handle(ctx)
            .update(ctx, |shell, ctx| shell.get_interactive_path_env_var(ctx));
        ctx.spawn(
            async move { probe_agent_commands(agent, cwd, path_env.await).await },
            move |me, commands, ctx| {
                if let Some(commands) = commands
                    && me.agent == Some(agent)
                {
                    me.set_agent_commands(commands, ctx);
                }
            },
        );
    }

    pub fn is_exact_agent_command(&self, text: &str) -> bool {
        let text = text.rsplit(char::is_whitespace).next().unwrap_or_default();
        self.state
            .agent_commands
            .iter()
            .any(|command| command.text == text)
    }

    pub(crate) fn set_agent_commands(
        &mut self,
        commands: Vec<AgentCommand>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.state.agent_commands != commands {
            self.state.agent_commands = commands;
            ctx.emit(UpdatedActiveCommands);
        }
    }

    pub fn set_active_repo_root(
        &mut self,
        repo_root: Option<PathBuf>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.update_active_repo_root(repo_root) {
            self.recompute_active_commands(ctx);
        }
    }

    pub(crate) fn command_is_active(&self, command: &StaticCommand, ctx: &AppContext) -> bool {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        self.command_passes_common_gates(command, availability, &gates)
    }

    fn recompute_active_commands(&mut self, ctx: &mut ModelContext<Self>) {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        let commands = HashMap::from_iter(
            COMMAND_REGISTRY
                .all_commands_by_id()
                .filter(|(_, command)| {
                    self.command_passes_common_gates(command, availability, &gates)
                })
                .map(|(id, command)| (id, command.clone())),
        );
        if self.replace_active_commands(commands) {
            ctx.emit(UpdatedActiveCommands);
        }
    }

    fn availability(&self, ctx: &AppContext) -> Availability {
        self.base_availability(ctx) | Availability::TERMINAL_VIEW
    }
}

impl SyncDataSource for GuiSlashCommandDataSource {
    type Action = AcceptSlashCommandOrSavedPrompt;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if query.text.is_empty() {
            return Ok(vec![]);
        }

        let query_text = query.text.trim().to_lowercase();
        let results = self.match_active_commands(&query_text, app);

        Ok(results
            .into_iter()
            .map(|item: InlineItem| item.into())
            .collect())
    }
}

impl SlashCommandDataSource for GuiSlashCommandDataSource {
    fn state(&self) -> &SlashCommandDataSourceState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut SlashCommandDataSourceState {
        &mut self.state
    }
}
impl Entity for GuiSlashCommandDataSource {
    type Event = UpdatedActiveCommands;
}
