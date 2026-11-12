use std::path::PathBuf;

use owo_colors::OwoColorize;

use crate::Context;
use crate::run_turn;
use crate::{
    AgentError, InputItem, InputMessageContent, LlmClient, ReasoningText, Role, TextContentPart,
    tools::ToolSpec,
};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

pub async fn repl() -> Result<(), AgentError> {
    let mut ctx = Context::new()?;

    let client = LlmClient::new(&ctx.config)?;

    let mut editor = DefaultEditor::new().map_err(|e| AgentError::new(e.to_string()))?;
    let repl_history = PathBuf::from(".history");
    let _ = editor.load_history(&repl_history);

    loop {
        let line = match editor.readline("› ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!();
                continue;
            },
            Err(ReadlineError::Eof) => {
                println!();
                break;
            },
            Err(err) => return Err(AgentError::new(err.to_string())),
        };

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        let _ = editor.add_history_entry(input);
        let _ = editor.save_history(&repl_history);

        match input {
            "/exit" | "/quit" => break,
            "/history" => {
                let items = &ctx.history.iter().collect::<Vec<&InputItem>>();
                print_input_items(items);
                continue;
            },
            "/tools" => {
                print_tools(&ctx.tool_specs);
                continue;
            },
            "/calls" => {
                let calls_and_outputs = ctx
                    .history
                    .iter()
                    .filter_map(|item| {
                        if matches!(
                            item,
                            InputItem::FunctionCall { .. } | InputItem::FunctionCallOutput { .. }
                        ) {
                            Some(item)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<&InputItem>>();
                print_input_items(&calls_and_outputs);
                continue;
            },
            _ => {},
        }

        let checkpoint = ctx.history.len();

        ctx.history.push(InputItem::Message {
            role: Role::User,
            content: InputMessageContent::Text(input.to_string()),
        });

        if let Err(e) = run_turn(&client, &mut ctx).await {
            print_error(&e.0);
            ctx.history.truncate(checkpoint);
        }
    }

    Ok(())
}

fn print_error(msg: &str) {
    eprintln!("{} {} : {}", "✗".red().bold(), "error".red().bold(), msg);
}

fn print_tools(specs: &[ToolSpec]) {
    let mut specs = specs.iter().collect::<Vec<&ToolSpec>>();
    specs.sort_by_key(|spec| spec.name);
    let width = specs.iter().map(|spec| spec.name.len()).max().unwrap_or(0);
    for spec in specs {
        let name = format!("{:<width$}", spec.name.cyan().bold());
        println!("{} {} {}", "tool".bright_black(), name, spec.snippet.dimmed());
    }
}

fn print_input_items(items: &[&InputItem]) {
    for item in items {
        let line = format!("{}", item.to_debug_colored());
        println!("{}", line.bright_black());
    }
}

impl InputItem {
    pub fn to_debug_colored(&self) -> String {
        match self {
            InputItem::Message { role, content } => {
                let role_str = match role {
                    Role::User => "user".blue().to_string(),
                    Role::Assistant => "assistant".green().to_string(),
                    Role::System => "system".yellow().to_string(),
                };
                format!("{} {}: {}", "message".bright_black(), role_str, content_colored(content),)
            },
            InputItem::Reasoning { content } => {
                let body = content
                    .iter()
                    .map(|r| match r {
                        ReasoningText::ReasoningText { text } => text.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{} {}", "reasoning".magenta().dimmed(), body.magenta().dimmed(),)
            },
            InputItem::FunctionCall { call_id, name, arguments } => {
                format!(
                    "{} {}({}) {}",
                    "function_call".cyan(),
                    name.cyan().bold(),
                    arguments.to_string().dimmed(),
                    format!("[{call_id}]").bright_black(),
                )
            },
            InputItem::FunctionCallOutput { call_id, output } => {
                format!(
                    "{} {} {}",
                    "function_call_output".bright_black(),
                    format!("[{call_id}]").bright_black(),
                    output.dimmed(),
                )
            },
        }
    }
}

fn content_colored(content: &InputMessageContent) -> String {
    match content {
        InputMessageContent::Text(s) => s.clone(),
        InputMessageContent::ContentParts(parts) => parts
            .iter()
            .map(|p| match p {
                TextContentPart::InputText { text } => text.clone(),
                TextContentPart::OutputText { text } => text.dimmed().to_string(),
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}
