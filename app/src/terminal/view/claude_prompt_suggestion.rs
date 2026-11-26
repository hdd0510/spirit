use warpui::{SingletonEntity as _, ViewContext};

use super::TerminalView;
use crate::editor::AutosuggestionType;
use crate::settings::AppEditorSettings;
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::model::cell::Flags;
use crate::terminal::model::grid::Dimensions as _;
use crate::terminal::model::grid::grid_handler::GridHandler;

const PROMPT_MARKER: char = '❯';
const BORDER: char = '─';
const PROMPT_TEXT_COLUMN: usize = 2;
const MAX_SUGGESTION_ROWS: usize = 6;
const PLACEHOLDER_PREFIXES: &[&str] = &[
    "Try \"",
    "Press up to ",
    "Message @",
    "Taking your message back",
    "Type your message while",
    "Describe something to",
    "Or describe something to",
];

impl TerminalView {
    pub(super) fn sync_claude_prompt_suggestion(&mut self, ctx: &mut ViewContext<Self>) {
        let suggestion = self.read_claude_prompt_suggestion(ctx);
        if suggestion == self.claude_prompt_suggestion {
            return;
        }
        self.dismiss_claude_prompt_suggestion(ctx);
        self.claude_prompt_suggestion = None;

        let Some(suggestion) = suggestion else {
            return;
        };
        if !self.input.as_ref(ctx).buffer_text(ctx).is_empty() {
            return;
        }
        self.input.update(ctx, |input, ctx| {
            input.set_autosuggestion(
                suggestion.clone(),
                AutosuggestionType::Command {
                    was_intelligent_autosuggestion: true,
                },
                ctx,
            );
        });
        self.claude_prompt_suggestion = Some(suggestion);
    }

    pub(super) fn dismiss_claude_prompt_suggestion(&mut self, ctx: &mut ViewContext<Self>) {
        if self.claude_prompt_suggestion.is_none() {
            return;
        }
        let editor = self.input.as_ref(ctx).editor().clone();
        editor.update(ctx, |editor, ctx| {
            if editor.active_autosuggestion_type()
                == Some(&AutosuggestionType::Command {
                    was_intelligent_autosuggestion: true,
                })
            {
                editor.clear_autosuggestion(ctx);
            }
        });
    }

    fn read_claude_prompt_suggestion(&self, ctx: &ViewContext<Self>) -> Option<String> {
        let sessions = CLIAgentSessionsModel::as_ref(ctx);
        let is_claude = sessions
            .session(self.view_id)
            .is_some_and(|session| session.agent == CLIAgent::Claude);
        if !is_claude
            || !sessions.is_input_open(self.view_id)
            || !*AppEditorSettings::as_ref(ctx).enable_autosuggestions
        {
            return None;
        }

        let model = self.model.lock();
        if model.is_alt_screen_active() {
            extract_prompt_suggestion(model.alt_screen().grid_handler())
        } else {
            extract_prompt_suggestion(
                model
                    .block_list()
                    .active_block()
                    .output_grid()
                    .grid_handler(),
            )
        }
    }
}

fn extract_prompt_suggestion(grid: &GridHandler) -> Option<String> {
    let last = grid.total_rows();
    let first = last.saturating_sub(grid.visible_rows());
    let prompt_row = (first + 1..last)
        .rev()
        .find(|&index| grid.row(index).is_some_and(|row| row[0].c == PROMPT_MARKER))?;
    if !is_border(grid, prompt_row - 1) {
        return None;
    }

    let mut lines = Vec::new();
    for index in prompt_row..=prompt_row + MAX_SUGGESTION_ROWS {
        if index > prompt_row && is_border(grid, index) {
            return finish_suggestion(&lines);
        }
        lines.push(dim_line(grid, index, index == prompt_row)?);
    }
    None
}

fn is_border(grid: &GridHandler, index: usize) -> bool {
    grid.row(index).is_some_and(|row| row[0].c == BORDER)
}

fn dim_line(grid: &GridHandler, index: usize, is_prompt_row: bool) -> Option<String> {
    let row = grid.row(index)?;
    if grid.columns() <= PROMPT_TEXT_COLUMN {
        return None;
    }
    let is_blank = |column: usize| row[column].c == '\0' || row[column].c.is_whitespace();
    if !is_prompt_row && !(0..PROMPT_TEXT_COLUMN).all(is_blank) {
        return None;
    }
    let mut line = String::new();
    for column in PROMPT_TEXT_COLUMN..grid.columns() {
        let cell = &row[column];
        if cell
            .flags()
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let is_cursor =
            is_prompt_row && column == PROMPT_TEXT_COLUMN && cell.flags().contains(Flags::INVERSE);
        let is_blank = is_blank(column);
        if !is_blank && !is_cursor && !cell.flags().contains(Flags::DIM) {
            return None;
        }
        if is_blank {
            line.push(' ');
        } else {
            line.push_str(&cell.raw_content().to_string());
        }
    }
    Some(line.trim().to_owned())
}

fn finish_suggestion(lines: &[String]) -> Option<String> {
    let text = lines.join(" ").trim().to_owned();
    let is_placeholder = PLACEHOLDER_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix));
    (!text.is_empty() && !text.ends_with('…') && !is_placeholder).then_some(text)
}
