//! What of zorite-editor shellwing keeps off. Obsidian's constructs
//! (`[[wiki-links]]`, `#tags`, `^block-ids`, `key:: value` properties,
//! `![[embeds]]`) read wrong in notes about servers, where `#` begins a
//! comment and `::` is a path; math (`$…$`, `$$…$$`) and mermaid have no
//! renderer here, and `$` is a shell's. Every place the editor recognizes
//! them asks here first; off, they are plain text.
//!
//! The code behind them is taken out once nothing reaches it.

use std::ops::Range;

use zorite_markdown::syntax::{self, LinkHit};

/// Wiki-links, tags, block ids, properties, embeds.
pub(crate) const OBSIDIAN: bool = false;
/// `$…$` and `$$…$$` math, mermaid diagrams.
pub(crate) const MATH: bool = false;

pub(crate) fn block_id(line: &str) -> Option<(usize, &str)> {
    syntax::block_id(line).filter(|_| OBSIDIAN)
}

pub(crate) fn embed_line(line: &str) -> Option<&str> {
    syntax::embed_line(line).filter(|_| OBSIDIAN)
}

pub(crate) fn prefixed_property(line: &str) -> Option<(&str, &str, &str)> {
    syntax::prefixed_property(line).filter(|_| OBSIDIAN)
}

/// The links of a line: a wiki-link or a tag (a "page") only with
/// Obsidian's constructs on.
pub(crate) fn links(line: &str) -> Vec<(Range<usize>, LinkHit)> {
    syntax::links(line)
        .into_iter()
        .filter(|(_, h)| OBSIDIAN || !matches!(h, LinkHit::Page(_)))
        .collect()
}

pub(crate) fn link_at(line: &str, col: usize) -> Option<LinkHit> {
    syntax::link_at(line, col).filter(|h| OBSIDIAN || !matches!(h, LinkHit::Page(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_about_servers_read_as_written() {
        // A shell comment, a path, a PID: no tag, no property, no math.
        assert!(links("ps -p $$ # check std::env first").is_empty());
        assert!(links("see [[Runbook]] and #ops").is_empty());
        assert!(prefixed_property("owner:: ops").is_none());
        assert!(block_id("restart it ^step1").is_none());
        assert!(embed_line("![[Other note]]").is_none());
        // A real link still is one.
        assert!(matches!(
            links("docs: [here](https://x.y)").as_slice(),
            [(_, LinkHit::Url(_))]
        ));
        // A note loads as it is, `$$` and all.
        let t = "kill $$\n$$\nnot math\n$$\n".to_string();
        assert_eq!(crate::normalize_loaded(t.clone()), t);
    }
}
