//! Miffy and ADK's single-agent loop: call the model with the session
//! history, run every function call it asks for, feed the responses back,
//! and stop at a reply without calls.

use std::future::Future;
use std::pin::Pin;

use crate::llm::{Llm, Meta};
use crate::model::{Content, FunctionResponse, Part, Request, Role};
use crate::tools::{self, Approval, Toolbox};

pub const NAME: &str = "Miffy";
pub const DESCRIPTION: &str = "YouTube growth strategist and workflow assistant — retrieve, create, update, and delete YouTube content.";
pub const INSTRUCTION: &str = include_str!("../assets/INSTRUCTION.md");

/// ADK gives up after this many consecutive thought-only replies.
const MAX_THOUGHT_ONLY_TURNS: usize = 10;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A human's answer to an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Decline,
    /// Decline this call and every later one in the reply, then end the turn
    /// once the function responses are in the history, without calling the
    /// model again: the human moved on to a new message.
    Stop,
}

/// Where a turn's output goes and where approvals come from: the console
/// and the web launchers.
pub trait Frontend: Send {
    /// One chunk of a streamed model reply, as it arrives.
    fn chunk(&mut self, chunk: &Content, meta: &Meta);
    /// Each content the turn adds to the history after the user message: a
    /// whole model reply with its metadata, or the function responses to
    /// its calls with `None`.
    fn content(&mut self, content: &Content, meta: Option<&Meta>);
    /// Asks the human to approve a write: the call at `position` in the
    /// model's latest reply.
    fn approve<'a>(&'a mut self, approval: &'a Approval, position: usize)
    -> BoxFuture<'a, Verdict>;
}

pub struct Agent {
    llm: Llm,
    /// The resolved system instruction, or the error ADK reports each turn
    /// when the instruction template cannot be filled.
    system: Result<String, String>,
    tools: Toolbox,
}

impl Agent {
    /// `instruction` replaces the embedded one wholesale; empty keeps it.
    pub fn new(llm: Llm, tools: Toolbox, instruction: &str) -> Agent {
        let instruction = if instruction.is_empty() {
            INSTRUCTION
        } else {
            instruction
        };
        let system = system_instruction(instruction, tools.skill_instruction());
        Agent { llm, system, tools }
    }

    /// Runs one user message to the model's final reply. `history` is the
    /// session; it keeps the message and everything the turn adds, even when
    /// the turn fails, as ADK's session does.
    pub async fn turn(
        &self,
        history: &mut Vec<Content>,
        message: Content,
        stream: bool,
        frontend: &mut dyn Frontend,
    ) -> Result<(), String> {
        history.push(message);
        let system = self.system.as_deref().map_err(Clone::clone)?;
        let mut thought_only = 0;
        loop {
            let req = Request {
                system,
                contents: history,
                declarations: self.tools.declarations(),
                google_search: self.tools.google_search(),
            };
            let crate::llm::Reply {
                content: reply,
                meta,
            } = self
                .llm
                .generate(&req, stream, &mut |c: &Content, m: &Meta| {
                    frontend.chunk(c, m)
                })
                .await?;
            frontend.content(&reply, Some(&meta));
            let calls: Vec<_> = reply.calls().cloned().collect();
            let thought = reply.thought_only();
            if !reply.parts.is_empty() {
                history.push(reply);
            }
            if calls.is_empty() {
                thought_only = if thought { thought_only + 1 } else { 0 };
                if thought && thought_only < MAX_THOUGHT_ONLY_TURNS {
                    continue;
                }
                return Ok(());
            }
            let mut parts = Vec::with_capacity(calls.len());
            let mut stopped = false;
            for (position, call) in calls.into_iter().enumerate() {
                let response = match self.tools.approval(&call) {
                    Some(_) if stopped => tools::declined(),
                    Some(a) => match frontend.approve(&a, position).await {
                        Verdict::Approve => self.tools.run(&call, true).await,
                        Verdict::Decline => tools::declined(),
                        Verdict::Stop => {
                            stopped = true;
                            tools::declined()
                        }
                    },
                    None => self.tools.run(&call, false).await,
                };
                parts.push(Part::Response(FunctionResponse {
                    id: call.id,
                    name: call.name,
                    response,
                }));
            }
            let responses = Content {
                role: Role::User,
                parts,
            };
            frontend.content(&responses, None);
            history.push(responses);
            if stopped {
                return Ok(());
            }
        }
    }
}

/// The system prompt in ADK's order: the agent instruction, its identity,
/// then the skill toolset's block.
fn system_instruction(instruction: &str, skills: Option<String>) -> Result<String, String> {
    let instruction = inject_session_state(instruction).map_err(|e| {
        format!(
            "failed to append instructions: failed to inject session state into instruction: {e}"
        )
    })?;
    let identity = format!(
        "You are an agent. Your internal name is {}. The description about you is {}.",
        yutu_auth::log::quote(NAME),
        yutu_auth::log::quote(DESCRIPTION)
    );
    let mut parts: Vec<String> = Vec::new();
    if !instruction.is_empty() {
        parts.push(instruction);
    }
    parts.push(identity);
    parts.extend(skills);
    Ok(parts.join("\n\n"))
}

/// ADK's `InjectSessionState` against the console's always-empty session
/// state and absent artifact service.
fn inject_session_state(template: &str) -> Result<String, String> {
    // The matches of ADK's placeholder pattern `{+[^{}]*}+`, leftmost first.
    let b = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let (mut i, mut last) = (0, 0);
    while i < b.len() {
        if b[i] != b'{' {
            i += 1;
            continue;
        }
        let mut k = i;
        while k < b.len() && b[k] == b'{' {
            k += 1;
        }
        while k < b.len() && b[k] != b'{' && b[k] != b'}' {
            k += 1;
        }
        if k == b.len() || b[k] == b'{' {
            i = k;
            continue;
        }
        let mut end = k;
        while end < b.len() && b[end] == b'}' {
            end += 1;
        }
        out.push_str(&template[last..i]);
        out.push_str(&replace(&template[i..end])?);
        (i, last) = (end, end);
    }
    out.push_str(&template[last..]);
    Ok(out)
}

fn replace(matched: &str) -> Result<String, String> {
    let mut name = matched.trim_matches(['{', '}']).trim();
    let optional = name.ends_with('?');
    if optional {
        name = &name[..name.len() - 1];
    }
    if name.starts_with("artifact.") {
        return Err("artifact service is not initialized".into());
    }
    if !valid_state_name(name) {
        return Ok(matched.to_string());
    }
    if optional {
        Ok(String::new())
    } else {
        Err("state key does not exist".into())
    }
}

fn valid_state_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split(':').collect();
    match parts.as_slice() {
        [id] => is_identifier(id),
        [prefix, id] => matches!(*prefix, "app" | "user" | "temp") && is_identifier(id),
        _ => false,
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_instruction_matches_adk() {
        assert_eq!(
            system_instruction("Be brief.", None).unwrap(),
            "Be brief.\n\nYou are an agent. Your internal name is \"Miffy\". The description about you is \"YouTube growth strategist and workflow assistant — retrieve, create, update, and delete YouTube content.\"."
        );
    }

    #[test]
    fn placeholders_resolve_against_empty_state() {
        assert_eq!(inject_session_state("a {x?} b").unwrap(), "a  b");
        assert_eq!(
            inject_session_state("keep {not valid} {{1x}}").unwrap(),
            "keep {not valid} {{1x}}"
        );
        assert_eq!(inject_session_state("{ {x?}").unwrap(), "{ ");
        assert_eq!(
            inject_session_state("{name}").unwrap_err(),
            "state key does not exist"
        );
        assert_eq!(
            inject_session_state("{artifact.f}").unwrap_err(),
            "artifact service is not initialized"
        );
    }
}
