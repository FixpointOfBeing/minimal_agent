use std::path::{Path, PathBuf};
use std::fs;

use crate::tools::ToolSpec;

const TEMPLATE: &str = include_str!("../prompts/system.md");

/// Renders the system prompt for the given working directory and tool set,
///
/// The tool list and rules are derived from `tools`, never hard-coded, so the
/// prompt always matches the tools actually declared to the model.
pub fn build_system_prompt(cwd: &Path, tools: &[ToolSpec]) -> String {
    let tools_block = tools_section(tools);
    let tool_rules = tool_rules(tools);

    TEMPLATE
        .replace("{{TOOLS}}", tools_block.trim_end())
        .replace("{{TOOL_RULES}}", &tool_rules)
        .replace("{{PROJECT_CONTEXT}}", project_context(cwd).trim_end())
        .replace("{{CWD}}", &format!("current working directory: {}", &cwd.display().to_string()))
}

fn tools_section(tools: &[ToolSpec]) -> String {
    let mut section = String::from("<tools>\n");
    for tool in tools {
        section.push_str(&format!("- {}: {}\n", tool.name, tool.snippet));
    }
    section.push_str(
        "\nIn addition to the tools above, you may have access to other custom tools depending on the project.\n",
    );
    section.push_str("</tools>");
    section
}


/// Renders the tool-specific portion of the `<rules>` block.
fn tool_rules(tools: &[ToolSpec]) -> String {

    let mut rules = String::new();

    for tool in tools {
        for guideline in tool.guidelines {
            rules.push_str(&format!("- {guideline}\n"));
        }
    }
    rules
}

fn project_context(cwd: &Path) -> String {
    let Some(path) = find_agents_md(cwd) else {
        return String::new();
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return String::new();
    };

    format!(
        "<project_context>\nProject-specific instructions and guidelines:\n\n<project_instructions path=\"{}\">\n{}\n</project_instructions>\n</project_context>",
        path.display(),
        content.trim_end()
    )
}

/// Walks from `cwd` upwards and returns the nearest `AGENTS.md`.
fn find_agents_md(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .map(|dir| dir.join("AGENTS.md"))
        .find(|path| path.is_file())
}

