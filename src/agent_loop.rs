
use std::io;
use std::io::Write;

use crate::{
    AgentError, Input, InputItem, LlmClient, Payload, Reasoning,
    ResponseStatus, StreamEventKind, StreamEventResponse,
    api::{StreamEvent, ToolDefinition},Context,
    tools::{Call, ToolOutput, ToolSpec},
};

use futures_util::StreamExt;

use owo_colors::OwoColorize;
use serde_json::json;

/// Hard cap on model requests per turn, to break runaway tool loops.
pub const MAX_TOOL_ROUNDS: usize = 20;

fn tool_specs_to_defs(tool_specs: &[ToolSpec]) -> Vec<ToolDefinition> {
    tool_specs
        .iter()
        .map(|spec| ToolDefinition::Function {
            name: spec.name.to_string(),
            description: spec.description.to_string(),
            parameters: spec.parameters.clone(),
        })
        .collect()
}


pub async fn run_turn(client: &LlmClient, ctx: &mut Context) -> Result<Vec<InputItem>, AgentError> {
    let mut round = 0usize;
    let tool_defs = tool_specs_to_defs(&ctx.tool_specs);
    let mut calls_and_outputs = Vec::new();
    loop {
        round += 1;
        if round > MAX_TOOL_ROUNDS {
            return Err(AgentError::new("超出最大循环次数"));
        }
        let request = Payload {
            model: ctx.config.model.clone(),
            instructions: ctx.system_prompt.clone(),
            input: Input::Items(ctx.history.clone()),
            reasoning: Reasoning { effort: ctx.config.reasoning.clone() },
            stream: true,
            tools: tool_defs.clone(),
        };
        let mut stream = client.ask_stream(request).await?;

        while let Some(event) = stream.next().await {
            let event = event?;
            render(&event);

            let Some(response) = event.response else {
                continue;
            };

            if response.status != ResponseStatus::Completed {
                continue;
            }

            let tool_calls = parse_calls(ctx, &response, &mut calls_and_outputs)?;
            if tool_calls.is_empty() {
                return Ok(calls_and_outputs);
            }
            for call in tool_calls {
                let tool_output = execute_call(ctx, call).await;
                let msg = tooloutput_to_msg(tool_output);
                ctx.history.push(msg.clone());
                calls_and_outputs.push(msg);
            }
        }
    }
}

fn parse_calls(
    ctx: &mut Context,
    response: &StreamEventResponse,
    calls_and_outputs: &mut Vec<InputItem>,
) -> Result<Vec<Call>, AgentError> {
    let mut calls = Vec::new();
    let mut inputs = Vec::with_capacity(response.output.len());
    let mut input_func_calls = Vec::new();
    for item in response.output.clone() {
        let input = InputItem::from(item);
        if let InputItem::FunctionCall { call_id, name, arguments } = &input {
            let parsed = match serde_json::from_str::<serde_json::Value>(arguments) {
                Ok(value) => value,
                Err(e) => return Err(AgentError::new(e.to_string())),
            };
            calls.push(Call {
                call_id: call_id.to_string(),
                name: name.to_string(),
                arguments: parsed,
            });
            input_func_calls.push(input.clone());
        }
        inputs.push(input);
    }
    calls_and_outputs.extend(input_func_calls);
    ctx.history.extend(inputs);
    Ok(calls)
}

fn render(event: &StreamEvent) {
    match event.kind {
        StreamEventKind::OutputTextDelta => {
            if let Some(delta) = event.delta.clone() {
                print!("{}", delta.fg_rgb::<255, 255, 240>());
                io::stdout().flush().ok();
            }
        },
        StreamEventKind::ReasoningTextDelta => {
            if let Some(delta) = event.delta.clone() {
                print!("{}", delta.fg_rgb::<149, 163, 166>());
                io::stdout().flush().ok();
            }
        },
        StreamEventKind::ReasoningTextDone => {
            println!("\n");
            io::stdout().flush().ok();
        },
        StreamEventKind::OutputTextDone => {
            println!("\n");
            io::stdout().flush().ok();
        },
        StreamEventKind::OutputItemDone => {},
        StreamEventKind::Other(_) => {},
    }
}

async fn execute_call(ctx: &Context, call: Call) -> ToolOutput {
    let Some(tool) = ctx.registry.get(&call.name) else {
        return ToolOutput::err(&call, format!("Tool {} not found", call.name.clone()));
    };

    tool.execute(ctx, call).await
}

fn tooloutput_to_msg(tool_output: ToolOutput) -> InputItem {
    match tool_output {
        ToolOutput::Ok { call, output } => {
            InputItem::FunctionCallOutput { call_id: call.call_id, output: output.to_string() }
        },
        ToolOutput::Err { call, error } => InputItem::FunctionCallOutput {
            call_id: call.call_id,
            output: json!({ "error": error }).to_string(),
        },
    }
}
