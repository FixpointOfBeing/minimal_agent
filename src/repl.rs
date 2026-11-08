use futures_util::StreamExt;
use owo_colors::OwoColorize;
use std::path::PathBuf;

use crate::{
    Config, Input, InputItem, InputMessageContent, LlmClient, LlmError, Payload, Reasoning, Role,
    StreamEventKind,
};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

pub async fn repl(config: Config) -> Result<(), LlmError> {
    let client = LlmClient::new(&config.clone())?;
    let mut history: Vec<InputItem> = Vec::new();

    let mut editor = DefaultEditor::new().map_err(|e| LlmError(e.to_string()))?;
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
            Err(err) => return Err(LlmError(err.to_string())),
        };

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        let _ = editor.add_history_entry(input);
        match input {
            "/exit" | "/quit" => break,
            _ => {},
        }

        history.push(InputItem::Message {
            role: Role::User,
            content: InputMessageContent::Text(input.to_string()),
        });
        let request = Payload {
            model: config.model.clone(),
            instructions: config.system_prompt.clone(),
            input: Input::Items(history.clone()),
            reasoning: Reasoning { effort: config.reasoning.clone() },
            stream: true,
        };

        let mut stream = client.ask_stream(request).await.map_err(|e| LlmError(e.to_string()))?;

        while let Some(event) = stream.next().await {
            let event = event?;
            match event.kind {
                StreamEventKind::OutputTextDelta => {
                    if let Some(delta) = event.delta {
                        print!("{}", delta.fg_rgb::<255, 255, 240>())
                    }
                },
                StreamEventKind::ReasoningTextDelta => {
                    if let Some(delta) = event.delta {
                        print!("{}", delta.fg_rgb::<149, 163, 166>())
                    }
                },
                StreamEventKind::OutputItemDone => {
                    println!("\n");
                },
                StreamEventKind::Other(_) => {},
            }

            match event.response {
                Some(response) if !response.output.is_empty() => {
                    for item in response.output {
                        history.push(item.into());
                    }
                },
                _ => {},
            };
        }
    }

    let _ = editor.save_history(&repl_history);
    Ok(())
}
