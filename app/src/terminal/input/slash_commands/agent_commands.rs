use std::path::Path;

use serde_json::Value;

use crate::terminal::CLIAgent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommand {
    pub text: String,
    pub description: String,
}

impl AgentCommand {
    fn new(text: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            description: description.into(),
        }
    }
}

const CLAUDE_INTERACTIVE_COMMANDS: &[(&str, &str)] = &[
    ("/artifacts", "Browse your published and shared artifacts"),
    ("/background", "Send this session to the background"),
    ("/branch", "Branch the conversation at this point"),
    ("/btw", "Ask a side question without interrupting Claude"),
    ("/desktop", "Continue this session in Claude Desktop"),
    ("/exit", "Exit Claude Code"),
    ("/feedback", "Send feedback or report a bug"),
    (
        "/remote-control",
        "Control this session from your phone or claude.ai/code",
    ),
    ("/tasks", "View and manage background work"),
    ("/teleport", "Resume a cloud session here"),
    ("/terminal-setup", "Set up terminal key bindings"),
    (
        "/web-setup",
        "Set up cloud sessions with your GitHub account",
    ),
    ("/workflows", "Browse running and completed workflows"),
    ("/add-dir", "Add a working directory"),
    ("/export", "Export the conversation"),
    ("/help", "Show help"),
    ("/hooks", "View hooks"),
    ("/login", "Sign in"),
    ("/logout", "Sign out"),
    ("/memory", "Edit memory files"),
    ("/permissions", "Manage tool permissions"),
    ("/plugin", "Manage plugins"),
    ("/resume", "Resume a conversation"),
    ("/rewind", "Restore an earlier point"),
    ("/status", "Show status"),
    ("/theme", "Change the theme"),
];

const CODEX_COMMANDS: &[(&str, &str)] = &[
    ("/model", "Choose the model and reasoning effort"),
    ("/permissions", "Choose what Codex can do"),
    ("/review", "Review current changes"),
    ("/new", "Start a new chat"),
    ("/resume", "Resume a saved chat"),
    ("/init", "Create an AGENTS.md file"),
    ("/compact", "Summarize the conversation"),
    ("/diff", "Show the git diff"),
    ("/mention", "Mention a file"),
    ("/status", "Show session configuration"),
    ("/mcp", "List MCP tools"),
    ("/skills", "Use skills"),
];

const PANEL_COMMANDS: &[&str] = &[
    "/artifacts",
    "/desktop",
    "/exit",
    "/feedback",
    "/remote-control",
    "/tasks",
    "/teleport",
    "/terminal-setup",
    "/web-setup",
    "/workflows",
    "/agents",
    "/config",
    "/effort",
    "/help",
    "/hooks",
    "/login",
    "/mcp",
    "/memory",
    "/model",
    "/output-style",
    "/permissions",
    "/plugin",
    "/resume",
    "/review",
    "/rewind",
    "/skills",
    "/status",
    "/theme",
    "/usage",
];

pub fn agent_menu_query(buffer: &str) -> Option<&str> {
    let token_start = buffer.rfind(char::is_whitespace).map_or(0, |index| {
        index + buffer[index..].chars().next().map_or(1, char::len_utf8)
    });
    let token = &buffer[token_start..];
    if let Some(query) = token.strip_prefix('/') {
        return (token_start == 0 && !query.contains('/')).then_some(query);
    }
    let query = token.strip_prefix('$')?;
    let is_skill_like = query
        .chars()
        .next()
        .is_none_or(|c| c.is_alphabetic() || c == '_');
    (is_skill_like && !query.contains(['/', '(', '{', '$'])).then_some(token)
}

pub fn insert_agent_command(buffer: &str, text: &str) -> String {
    let token_start = buffer
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(index, c)| index + c.len_utf8());
    format!("{}{text} ", &buffer[..token_start])
}

pub fn rank_agent_command(command: &AgentCommand, query: &str) -> Option<u32> {
    let (query, skills_only) = match query.strip_prefix('$') {
        Some(query) => (query, true),
        None => (query, false),
    };
    if skills_only && !command.text.starts_with('$') {
        return None;
    }
    let query = query.to_lowercase();
    let name = command.text.get(1..)?.to_lowercase();
    let description = command.description.to_lowercase();
    let starts_word = |text: &str| {
        text.split(|c: char| !c.is_alphanumeric())
            .any(|word| word.starts_with(&query))
    };
    if query.is_empty() || name == query {
        Some(0)
    } else if name.starts_with(&query) {
        Some(1)
    } else if starts_word(&name) {
        Some(2)
    } else if name.contains(&query) {
        Some(3)
    } else if starts_word(&description) {
        Some(4)
    } else if query.chars().count() > 2 && description.contains(&query) {
        Some(5)
    } else {
        None
    }
}

pub fn opens_agent_panel(text: &str) -> bool {
    PANEL_COMMANDS.contains(&text.trim())
}

pub fn supports_agent_commands(agent: CLIAgent) -> bool {
    matches!(agent, CLIAgent::Claude | CLIAgent::Codex)
}

pub fn fallback_commands(agent: CLIAgent) -> Vec<AgentCommand> {
    match agent {
        CLIAgent::Codex => from_table(CODEX_COMMANDS),
        _ => vec![],
    }
}

fn from_table(table: &[(&str, &str)]) -> Vec<AgentCommand> {
    table
        .iter()
        .map(|(text, description)| AgentCommand::new(*text, *description))
        .collect()
}

pub fn parse_claude_commands(message: &Value) -> Option<Vec<AgentCommand>> {
    if message["type"] != "control_response" {
        return None;
    }
    let mut commands: Vec<AgentCommand> = message["response"]["response"]["commands"]
        .as_array()?
        .iter()
        .filter_map(|command| {
            let name = command["name"].as_str()?;
            (is_safe_name(name) && !name.starts_with("__")).then(|| {
                AgentCommand::new(
                    format!("/{name}"),
                    one_line(command["description"].as_str().unwrap_or_default()),
                )
            })
        })
        .collect();
    for extra in from_table(CLAUDE_INTERACTIVE_COMMANDS) {
        if !commands.iter().any(|command| command.text == extra.text) {
            commands.push(extra);
        }
    }
    Some(commands)
}

pub fn parse_codex_skills(message: &Value) -> Option<Vec<AgentCommand>> {
    if message["id"] != 2 {
        return None;
    }
    let mut commands = from_table(CODEX_COMMANDS);
    for skill in message["result"]["data"]
        .as_array()?
        .iter()
        .filter_map(|entry| entry["skills"].as_array())
        .flatten()
    {
        let Some(name) = skill["name"].as_str() else {
            continue;
        };
        let text = format!("${name}");
        if is_safe_name(name)
            && skill["enabled"] != false
            && !commands.iter().any(|command| command.text == text)
        {
            commands.push(AgentCommand::new(
                text,
                one_line(skill["description"].as_str().unwrap_or_default()),
            ));
        }
    }
    Some(commands)
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(|c: char| c.is_whitespace() || c.is_control())
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn is_trusted_by_claude(config: &Value, cwd: &Path) -> bool {
    cwd.ancestors().any(|dir| {
        dir.to_str()
            .is_some_and(|dir| config["projects"][dir]["hasTrustDialogAccepted"] == true)
    })
}

#[cfg(feature = "local_fs")]
pub async fn probe_agent_commands(
    agent: CLIAgent,
    cwd: std::path::PathBuf,
    path_env: Option<String>,
) -> Option<Vec<AgentCommand>> {
    use std::time::Duration;

    use command::Stdio;
    use command::r#async::Command;
    use futures::io::BufReader;
    use futures::{AsyncBufReadExt as _, AsyncWriteExt as _, StreamExt as _};
    use warpui::r#async::FutureExt as _;

    let mut command;
    let request: &str;
    let parse: fn(&Value) -> Option<Vec<AgentCommand>>;
    match agent {
        CLIAgent::Claude => {
            let config: Value = dirs::home_dir()
                .and_then(|home| std::fs::read_to_string(home.join(".claude.json")).ok())
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let sources = if is_trusted_by_claude(&config, &cwd) {
                "user,project,local"
            } else {
                "user"
            };
            command = Command::new("claude");
            command
                .args(["-p", "--input-format", "stream-json"])
                .args(["--output-format", "stream-json", "--verbose"])
                .args(["--strict-mcp-config", "--no-session-persistence"])
                .args(["--setting-sources", sources])
                .args(["--settings", r#"{"disableAllHooks":true}"#])
                .env_remove("CLAUDECODE");
            request = "{\"type\":\"control_request\",\"request_id\":\"1\",\"request\":{\"subtype\":\"initialize\"}}\n";
            parse = parse_claude_commands;
        }
        CLIAgent::Codex => {
            command = Command::new("codex");
            command.arg("app-server");
            request = concat!(
                "{\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"terminal\",\"version\":\"1\"}}}\n",
                "{\"method\":\"initialized\"}\n",
                "{\"id\":2,\"method\":\"skills/list\",\"params\":{}}\n",
            );
            parse = parse_codex_skills;
        }
        _ => return None,
    }
    if let Some(path_env) = path_env {
        command.env("PATH", path_env);
    }
    let mut child = command
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    async {
        stdin.write_all(request.as_bytes()).await.ok()?;
        stdin.flush().await.ok()?;
        let mut lines = BufReader::new(stdout).lines();
        while let Some(Ok(line)) = lines.next().await {
            let commands = serde_json::from_str(&line)
                .ok()
                .and_then(|message| parse(&message));
            if commands.is_some() {
                return commands;
            }
        }
        None
    }
    .with_timeout(Duration::from_secs(15))
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
#[path = "agent_commands_tests.rs"]
mod tests;
