//! Miffy's tools: the 74 youtube tools reached through an in-process MCP
//! session (so validation, defaults and confirmation gating are youtube-mcp's),
//! the embedded skill toolset, and Gemini's built-in Google Search.

use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use youtube_core::ops::ServiceProvider;
use youtube_core::registry;
use youtube_mcp::{Incoming, Server, Session};

use crate::model::{Declaration, FunctionCall};
use crate::skills::{self, Skills};

pub const GOOGLE_SEARCH: &str = "google_search";

/// A write the model asked for, shown to the human before it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    pub tool: String,
    /// The arguments as Go's `json.Marshal` writes them.
    pub args: String,
    /// The op's own confirmation message, empty when it has none or the
    /// arguments do not bind.
    pub message: String,
}

pub struct Toolbox {
    session: Arc<Session>,
    skills: Skills,
    declarations: Vec<Declaration>,
    google_search: bool,
    next_id: AtomicI64,
}

/// ADK's function response for a call the human declined: what Go's MCP
/// toolset returns when an elicitation is declined.
pub fn declined() -> Map<String, Value> {
    error("Tool execution failed. Details: canceled by user".into())
}

fn error(text: String) -> Map<String, Value> {
    Map::from_iter([("error".to_string(), Value::String(text))])
}

impl Toolbox {
    pub async fn new(
        provider: Arc<dyn ServiceProvider>,
        google_search: bool,
    ) -> Result<Toolbox, String> {
        let session = Session::new(Server::new(provider, youtube_mcp::VERSION), None);
        let mut tb = Toolbox {
            session,
            skills: Skills::visible(),
            declarations: Vec::new(),
            google_search,
            next_id: AtomicI64::new(1),
        };
        tb.rpc(
            "initialize",
            json!({"protocolVersion": "2025-11-25", "capabilities": {},
                   "clientInfo": {"name": "youtube-agent", "version": youtube_mcp::VERSION}}),
        )
        .await?;
        let listed = tb.rpc("tools/list", json!({})).await?;
        let tools = listed["tools"].as_array().cloned().unwrap_or_default();
        tb.declarations = tools
            .iter()
            .map(|t| Declaration {
                name: t["name"].as_str().unwrap_or_default().to_string(),
                description: t["description"].as_str().unwrap_or_default().to_string(),
                parameters: t["inputSchema"].clone(),
                response: None,
            })
            .chain(Skills::declarations())
            .collect();
        Ok(tb)
    }

    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    pub fn google_search(&self) -> bool {
        self.google_search
    }

    pub fn skill_instruction(&self) -> Option<String> {
        self.skills.instruction()
    }

    /// Divergence E9: every tool whose registry effect is not Read needs a
    /// human, whatever the model sends. Arguments that fail validation skip
    /// the question: the call returns the validation error and runs nothing.
    pub fn approval(&self, call: &FunctionCall) -> Option<Approval> {
        let (_, tool) = registry::tools().find(|(_, t)| t.name == call.name)?;
        if !tool.gated() {
            return None;
        }
        let args = Value::Object(call.args.clone()).to_string();
        let bound = youtube_mcp::tools::find(&call.name)?.bind(Some(&args)).ok()?;
        let message = youtube_core::confirm::printable(&bound.invoke.confirm());
        Some(Approval {
            tool: call.name.clone(),
            args: crate::gojson::marshal(&Value::Object(call.args.clone())),
            message,
        })
    }

    /// Runs one call and returns the function response ADK would build.
    /// `approved` answers youtube-mcp's confirmation round trip for a call the
    /// human already approved.
    pub async fn run(&self, call: &FunctionCall, approved: bool) -> Map<String, Value> {
        if skills::TOOL_NAMES.contains(&call.name.as_str()) {
            return self
                .skills
                .call(&call.name, &call.args)
                .unwrap_or_else(error);
        }
        if youtube_mcp::tools::find(&call.name).is_none() {
            return error(self.not_found(&call.name));
        }
        let mut params = json!({"name": call.name, "arguments": call.args});
        if approved {
            params["inputResponses"] = json!({"confirm": {"action": "accept"}});
        }
        match self.rpc("tools/call", params).await {
            Ok(result) => {
                let text: String = result["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["text"].as_str())
                    .collect();
                if result["isError"] == Value::Bool(true) {
                    let mut msg = "Tool execution failed.".to_string();
                    if !text.is_empty() {
                        msg.push_str(" Details: ");
                        msg.push_str(&text);
                    }
                    error(msg)
                } else {
                    Map::from_iter([("output".to_string(), Value::String(text))])
                }
            }
            Err(e) => error(format!(
                "failed to call MCP tool {} with err: {e}",
                youtube_auth::log::quote(&call.name)
            )),
        }
    }

    /// ADK's tool-not-found error, listing tools in declaration order where
    /// Go lists them in map order.
    fn not_found(&self, name: &str) -> String {
        let names: Vec<&str> = self
            .google_search
            .then_some(GOOGLE_SEARCH)
            .into_iter()
            .chain(self.declarations.iter().map(|d| d.name.as_str()))
            .collect();
        format!(
            "tool '{name}' not found.
Available tools: {}

Possible causes:
  1. LLM hallucinated the function name - review agent instruction clarity
  2. Tool not registered - verify agent.tools list
  3. Name mismatch - check for typos

Suggested fixes:
  - Review agent instruction to ensure tool usage is clear
  - Verify tool is included in agent.tools list
  - Check for typos in function name",
            names.join(", ")
        )
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let msg = match Incoming::parse(&frame.to_string()) {
            Ok(Incoming::Single(m)) => m,
            _ => return Err(format!("encoding {method} request")),
        };
        let line = self
            .session
            .handle(msg)
            .await
            .ok_or_else(|| format!("no reply to {method}"))?;
        let mut reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if let Some(err) = reply.get("error") {
            return Err(err["message"].as_str().unwrap_or_default().to_string());
        }
        Ok(reply["result"].take())
    }
}
