use std::path::Path;

use serde_json::json;

use super::*;

fn texts(commands: Option<Vec<AgentCommand>>) -> Vec<String> {
    commands
        .expect("commands")
        .into_iter()
        .map(|command| command.text)
        .collect()
}

#[test]
fn claude_commands_come_from_the_initialize_response() {
    let message = json!({"type": "control_response", "response": {"response": {"commands": [
        {"name": "code-review:code-review", "description": "Review\n a PR"},
        {"name": "__internal", "description": ""},
        {"name": "bad name", "description": ""},
        {"name": "status", "description": "From Claude"},
    ]}}});
    let commands = parse_claude_commands(&message).unwrap();
    assert_eq!(commands[0].text, "/code-review:code-review");
    assert_eq!(commands[0].description, "Review a PR");
    let texts = texts(Some(commands));
    assert!(texts.contains(&"/resume".to_owned()));
    assert!(texts.contains(&"/remote-control".to_owned()));
    assert!(!texts.contains(&"/__internal".to_owned()));
    assert!(!texts.contains(&"/bad name".to_owned()));
    assert_eq!(texts.iter().filter(|text| *text == "/status").count(), 1);
    assert!(parse_claude_commands(&json!({"type": "system"})).is_none());
}

#[test]
fn codex_lists_enabled_skills_after_its_commands() {
    let message = json!({"id": 2, "result": {"data": [{"skills": [
        {"name": "ship", "description": "Ship it", "enabled": true},
        {"name": "off", "description": "", "enabled": false},
        {"name": "ship", "description": "Duplicate", "enabled": true},
    ]}]}});
    let texts = texts(parse_codex_skills(&message));
    assert_eq!(texts.first().map(String::as_str), Some("/model"));
    assert_eq!(texts.iter().filter(|text| *text == "$ship").count(), 1);
    assert!(!texts.contains(&"$off".to_owned()));
    assert!(parse_codex_skills(&json!({"id": 1, "result": {}})).is_none());
}

#[test]
fn project_settings_need_claude_trust() {
    let config = json!({"projects": {
        "/a/trusted": {"hasTrustDialogAccepted": true},
        "/a/declined": {"hasTrustDialogAccepted": false},
    }});
    assert!(is_trusted_by_claude(&config, Path::new("/a/trusted/sub")));
    assert!(!is_trusted_by_claude(&config, Path::new("/a/declined")));
    assert!(!is_trusted_by_claude(&config, Path::new("/a")));
    assert!(!is_trusted_by_claude(&json!(null), Path::new("/a/trusted")));
}

#[test]
fn only_bare_panel_commands_hand_the_keyboard_to_the_agent() {
    assert!(opens_agent_panel("/model "));
    assert!(opens_agent_panel("/remote-control"));
    assert!(!opens_agent_panel("/model opus"));
    assert!(!opens_agent_panel("/compact"));
    assert!(!opens_agent_panel("model"));
}

#[test]
fn slash_opens_at_the_start_and_skills_open_anywhere() {
    assert_eq!(agent_menu_query("/mod"), Some("mod"));
    assert_eq!(agent_menu_query("/"), Some(""));
    assert_eq!(agent_menu_query("$sh"), Some("$sh"));
    assert_eq!(agent_menu_query("please use $sh"), Some("$sh"));
    for text in [
        "see /tmp",
        "/model opus",
        "/usr/bin",
        "costs $5",
        "$(pwd)",
        "plain",
        "",
    ] {
        assert_eq!(agent_menu_query(text), None, "{text}");
    }
    assert_eq!(insert_agent_command("dùng $sh", "$ship"), "dùng $ship ");
    assert_eq!(insert_agent_command("/mod", "/model"), "/model ");
}

#[test]
fn names_rank_above_descriptions_and_dollar_lists_only_skills() {
    let command = |text: &str, description: &str| AgentCommand::new(text, description);
    let rank = |text, description, query| rank_agent_command(&command(text, description), query);
    assert!(rank("/review", "", "review") < rank("/review-pr", "", "review"));
    assert!(rank("/review-pr", "", "review") < rank("/code-review", "", "review"));
    assert!(rank("/code-review", "", "review") < rank("/check", "Review a PR", "review"));
    assert_eq!(rank("/check", "Nothing", "review"), None);
    assert_eq!(rank("/ship", "", "$ship"), None);
    assert!(rank("$ship", "", "$sh").is_some());
    assert!(rank("$ship", "", "$").is_some());
}

#[cfg(feature = "local_fs")]
#[test]
#[ignore = "runs the installed agents"]
fn installed_agents_list_their_commands() {
    let cwd = std::env::current_dir().unwrap();
    for (agent, expected) in [
        (CLIAgent::Claude, "/remote-control"),
        (CLIAgent::Codex, "/model"),
    ] {
        let commands = warpui::r#async::block_on(probe_agent_commands(agent, cwd.clone(), None));
        let texts = texts(commands);
        assert!(texts.contains(&expected.to_owned()), "{agent:?}: {texts:?}");
        println!("{agent:?}: {}", texts.len());
    }
}
