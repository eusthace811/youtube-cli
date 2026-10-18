//! `xtask skillgen`: the agent skill and instruction, generated from the
//! registry so they name only real tools, each under its registry effect
//! (divergence E13). Port of Go's internal/tools/skillgen.

use crate::emit::{Generated, emit};
use crate::registry::{Effect, Registry};
use std::fmt::Write;

const SETUP: &str = include_str!("../skill-refs/setup.md");
const WORKFLOWS: &str = include_str!("../skill-refs/workflows.md");
const SEO_GUIDE: &str = include_str!("../skill-refs/seo-guide.md");

const DESCRIPTION: &str = "Use whenever the user mentions YouTube, video uploads, channel management, playlists, video SEO, or any YouTube Data API operation. Manages videos, playlists, comments, captions, subscriptions, thumbnails, analytics, and more.";

const COMPATIBILITY: &str = "Requires the youtube CLI binary from youtube-cli (installable via npm, its install script, or from source) and Google Cloud OAuth credentials for YouTube Data API v3.";

const SAFETY_RULES: &str = "- **Confirm before deleting**: Always list exactly what will be deleted and get explicit user confirmation before calling any destructive tool.
- **Verify targets**: Before modifying or deleting, use a read tool to confirm the resource exists and matches the user's intent.
- **Preserve existing data**: When updating resources, only modify fields the user explicitly asked to change.
- **Report resource IDs**: Always include resource IDs in responses so the user can reference them.";

const GROWTH_TIPS: &str = "- **Titles**: Curiosity gaps + power words. Front-load keywords. Under 60 characters.
- **Descriptions**: First 2 lines appear in search. Include keywords, timestamps, CTAs, 3-5 hashtags.
- **Tags**: Mix broad and long-tail keywords. First 2-3 tags carry the most weight.
- **Thumbnails**: High contrast, 3-4 word text, expressive faces, consistent branding.
- **Publishing**: Post when audience is active. Consistent schedule matters.
- **Engagement**: Pin a comment with a question. Reply within the first hour.";

const INSTRUCTION: &str = "You are an expert YouTube growth strategist and workflow assistant. You are authenticated to operate YouTube channels on behalf of users via tool calls.

Consult the **youtube** skill for the full tool catalog, safety rules, workflows, and SEO guidelines.

## Workflow

For every user request:

1. **Classify**: Is this a read-only query, a content modification, a deletion, or a multistep workflow?
2. **Gather context**: Use read tools to find resource IDs, verify targets, and understand current state.
3. **Execute**: Use write or delete tools with the gathered data.
4. **Report**: State what was done, including resource IDs and titles.
";

/// One SKILL.md section per effect, in output order.
const SECTIONS: [(Effect, &str); 4] = [
    (Effect::Read, "Read (safe, no side effects)"),
    (Effect::Write, "Write (create or update content)"),
    (
        Effect::WritesLocalFile,
        "Local file (writes a file on this machine — confirm the path with user first)",
    ),
    (
        Effect::Destructive,
        "Destructive (irreversible deletions — confirm with user first)",
    ),
];

/// The external skill ships setup instructions; the agent's embedded copy
/// runs inside an installed youtube and does not.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    External,
    Agent,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let registry = Registry::load(&crate::repo_root())?;
    emit(&generate(&registry), args, "skillgen")
}

fn generate(registry: &Registry) -> Vec<Generated> {
    let file = |path: &'static str, text: String| Generated {
        path,
        text,
        executable: false,
    };
    vec![
        file("skills/youtube/SKILL.md", skill(registry, Target::External)),
        file("skills/youtube/references/setup.md", SETUP.into()),
        file("skills/youtube/references/workflows.md", WORKFLOWS.into()),
        file("skills/youtube/references/seo-guide.md", SEO_GUIDE.into()),
        // Go writes the agent's copies under cmd/agent. They move to
        // crates/youtube-agent/assets once that crate embeds them.
        file(
            "crates/youtube-agent/assets/INSTRUCTION.md",
            INSTRUCTION.into(),
        ),
        file(
            "crates/youtube-agent/assets/skills/youtube/SKILL.md",
            skill(registry, Target::Agent),
        ),
        file(
            "crates/youtube-agent/assets/skills/youtube/references/workflows.md",
            WORKFLOWS.into(),
        ),
        file(
            "crates/youtube-agent/assets/skills/youtube/references/seo-guide.md",
            SEO_GUIDE.into(),
        ),
    ]
}

fn skill(registry: &Registry, target: Target) -> String {
    // Sorted by resource, then verb, as Go sorts them.
    let tools: Vec<(&str, Effect)> = registry
        .resources()
        .into_iter()
        .flat_map(|r| registry.children(&r.path))
        .filter_map(|c| c.tool.as_ref())
        .map(|t| (t.name.as_str(), t.effect))
        .collect();
    let mut resources: Vec<&str> = tools
        .iter()
        .map(|(name, _)| name.split('-').next().unwrap())
        .collect();
    resources.dedup();

    let mut b = String::new();
    // The repository and every artifact are Apache-2.0; Go's `license: MIT`
    // misstated the license of a file it ships under Apache-2.0.
    write!(
        b,
        "---\nname: youtube\ndescription: \"{DESCRIPTION}\"\nlicense: Apache-2.0\n"
    )
    .unwrap();
    if target == Target::External {
        writeln!(b, "compatibility: {COMPATIBILITY}").unwrap();
    }
    b.push_str("metadata:\n  author: youtube-cli\n---\n\n");
    b.push_str("# YouTube\n\n");
    writeln!(
        b,
        "Manage YouTube resources via MCP tool calls — {}.\n",
        resources.join(", ")
    )
    .unwrap();
    if target == Target::External {
        b.push_str("## Setup\n\nEnsure `youtube` is installed and running as an MCP server. See [references/setup.md](references/setup.md).\n\n");
    }
    writeln!(b, "## Key Principles\n\n{SAFETY_RULES}\n").unwrap();
    b.push_str("## Tools\n");
    for (effect, heading) in SECTIONS {
        writeln!(b, "\n### {heading}\n").unwrap();
        for (name, _) in tools.iter().filter(|(_, e)| *e == effect) {
            writeln!(b, "- `{name}`").unwrap();
        }
    }
    b.push_str("\n## Common Workflows\n\nSee [references/workflows.md](references/workflows.md) for step-by-step walkthroughs.\n\n");
    b.push_str("## YouTube Growth Tips\n\nSee [references/seo-guide.md](references/seo-guide.md) for the full guide. When creating or updating content, apply these principles:\n\n");
    writeln!(b, "{GROWTH_TIPS}").unwrap();
    b
}
