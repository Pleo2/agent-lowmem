use crate::{
    result::Reason,
    script::tokenizer::{CommandSegment, tokenize_script},
};
use serde::Serialize;
use std::collections::BTreeMap;

pub const MAX_REFERENCE_DEPTH: u8 = 3;
pub const MAX_LEAF_OCCURRENCES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptPhase {
    Pre,
    Target,
    Post,
    Referenced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafOccurrence {
    pub script_key: String,
    pub phase: ScriptPhase,
    pub potential_lifecycle: bool,
    pub depth: u8,
    pub final_top_level: bool,
    pub segment: CommandSegment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGraph {
    pub leaves: Vec<LeafOccurrence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceKind {
    Node,
    PackageManager,
}

pub fn expand_script_graph(
    target: &str,
    scripts: &BTreeMap<String, String>,
) -> Result<ScriptGraph, Reason> {
    if !scripts.contains_key(target) {
        return Err(Reason::ScriptReferenceUnsupported);
    }

    let mut expansion = Expansion {
        scripts,
        active: Vec::new(),
        leaves: Vec::new(),
    };
    expansion.visit(target, ScriptPhase::Target, false, 0, true)?;
    Ok(ScriptGraph {
        leaves: expansion.leaves,
    })
}

struct Expansion<'a> {
    scripts: &'a BTreeMap<String, String>,
    active: Vec<String>,
    leaves: Vec<LeafOccurrence>,
}

impl Expansion<'_> {
    fn visit(
        &mut self,
        script_key: &str,
        phase: ScriptPhase,
        potential_lifecycle: bool,
        depth: u8,
        include_lifecycle: bool,
    ) -> Result<(), Reason> {
        if self.active.iter().any(|active| active == script_key) {
            return Err(Reason::ScriptReferenceUnsupported);
        }
        let script = self
            .scripts
            .get(script_key)
            .ok_or(Reason::ScriptReferenceUnsupported)?;
        self.active.push(script_key.to_owned());

        let result = (|| {
            if include_lifecycle {
                let pre_key = format!("pre{script_key}");
                if self.scripts.contains_key(&pre_key) {
                    self.visit(&pre_key, ScriptPhase::Pre, true, depth, false)?;
                }
            }

            let tokenized = tokenize_script(script.as_bytes())?;
            let segment_count = tokenized.segments().len();
            for (segment_index, segment) in tokenized.segments().iter().enumerate() {
                match classify_reference(segment.arguments())? {
                    Some((kind, referenced_key)) => {
                        if depth >= MAX_REFERENCE_DEPTH
                            || !self.scripts.contains_key(referenced_key)
                        {
                            return Err(Reason::ScriptReferenceUnsupported);
                        }
                        self.visit(
                            referenced_key,
                            ScriptPhase::Referenced,
                            potential_lifecycle,
                            depth + 1,
                            kind == ReferenceKind::PackageManager,
                        )?;
                    }
                    None => self.push_leaf(LeafOccurrence {
                        script_key: script_key.to_owned(),
                        phase,
                        potential_lifecycle,
                        depth,
                        final_top_level: phase == ScriptPhase::Target
                            && !potential_lifecycle
                            && depth == 0
                            && segment_index + 1 == segment_count,
                        segment: segment.clone(),
                    })?,
                }
            }

            if include_lifecycle {
                let post_key = format!("post{script_key}");
                if self.scripts.contains_key(&post_key) {
                    self.visit(&post_key, ScriptPhase::Post, true, depth, false)?;
                }
            }
            Ok(())
        })();

        self.active.pop();
        result
    }

    fn push_leaf(&mut self, leaf: LeafOccurrence) -> Result<(), Reason> {
        if self.leaves.len() >= MAX_LEAF_OCCURRENCES {
            return Err(Reason::ScriptGraphTooLarge);
        }
        self.leaves.push(leaf);
        Ok(())
    }
}

fn classify_reference(arguments: &[String]) -> Result<Option<(ReferenceKind, &str)>, Reason> {
    let Some(executable) = arguments.first().map(String::as_str) else {
        return Err(Reason::ScriptReferenceUnsupported);
    };
    let kind = match (executable, arguments.get(1).map(String::as_str)) {
        ("node", Some("--run")) => Some(ReferenceKind::Node),
        ("node", Some(argument)) if argument.starts_with("--run") => {
            return Err(Reason::ScriptReferenceUnsupported);
        }
        ("npm" | "pnpm", Some("run")) => Some(ReferenceKind::PackageManager),
        ("npm" | "pnpm", _) => return Err(Reason::ScriptReferenceUnsupported),
        _ => None,
    };
    let Some(kind) = kind else {
        return Ok(None);
    };
    if arguments.len() != 3 {
        return Err(Reason::ScriptReferenceUnsupported);
    }
    let referenced_key = &arguments[2];
    if referenced_key.is_empty()
        || referenced_key.starts_with('-')
        || referenced_key.contains(['*', '?', '[', ']'])
    {
        return Err(Reason::ScriptReferenceUnsupported);
    }
    Ok(Some((kind, referenced_key)))
}

#[cfg(test)]
mod tests {
    use super::{MAX_LEAF_OCCURRENCES, ScriptPhase, expand_script_graph};
    use crate::result::Reason;
    use std::collections::BTreeMap;

    fn scripts(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn preserves_lifecycle_and_nested_package_reference_order() {
        let graph = expand_script_graph(
            "test",
            &scripts(&[
                ("pretest", "prepare"),
                ("test", "npm run nested"),
                ("posttest", "cleanup"),
                ("prenested", "nested-prepare"),
                ("nested", "runner --safe"),
                ("postnested", "nested-cleanup"),
            ]),
        )
        .unwrap();

        assert_eq!(
            graph
                .leaves
                .iter()
                .map(|leaf| (
                    leaf.script_key.as_str(),
                    leaf.phase,
                    leaf.potential_lifecycle,
                    leaf.depth,
                ))
                .collect::<Vec<_>>(),
            [
                ("pretest", ScriptPhase::Pre, true, 0),
                ("prenested", ScriptPhase::Pre, true, 1),
                ("nested", ScriptPhase::Referenced, false, 1),
                ("postnested", ScriptPhase::Post, true, 1),
                ("posttest", ScriptPhase::Post, true, 0),
            ]
        );
    }

    #[test]
    fn node_references_do_not_add_package_lifecycle_scripts() {
        let graph = expand_script_graph(
            "test",
            &scripts(&[
                ("test", "node --run nested"),
                ("prenested", "must-not-appear"),
                ("nested", "runner"),
                ("postnested", "must-not-appear"),
            ]),
        )
        .unwrap();

        assert_eq!(graph.leaves.len(), 1);
        assert_eq!(graph.leaves[0].script_key, "nested");
        assert_eq!(graph.leaves[0].phase, ScriptPhase::Referenced);
        assert!(!graph.leaves[0].potential_lifecycle);
    }

    #[test]
    fn permits_reference_depth_three_and_rejects_depth_four() {
        let accepted = scripts(&[
            ("a", "node --run b"),
            ("b", "node --run c"),
            ("c", "node --run d"),
            ("d", "runner"),
        ]);
        assert_eq!(
            expand_script_graph("a", &accepted).unwrap().leaves[0].depth,
            3
        );

        let rejected = scripts(&[
            ("a", "node --run b"),
            ("b", "node --run c"),
            ("c", "node --run d"),
            ("d", "node --run e"),
            ("e", "runner"),
        ]);
        assert_eq!(
            expand_script_graph("a", &rejected).unwrap_err(),
            Reason::ScriptReferenceUnsupported
        );
    }

    #[test]
    fn rejects_reference_cycles() {
        let cycle = scripts(&[("a", "pnpm run b"), ("b", "npm run a")]);
        assert_eq!(
            expand_script_graph("a", &cycle).unwrap_err(),
            Reason::ScriptReferenceUnsupported
        );
    }

    #[test]
    fn charges_repeated_references_as_separate_leaf_occurrences() {
        let graph = expand_script_graph(
            "test",
            &scripts(&[("test", "npm run leaf && npm run leaf"), ("leaf", "runner")]),
        )
        .unwrap();

        assert_eq!(graph.leaves.len(), 2);
        assert_eq!(graph.leaves[0], graph.leaves[1]);
    }

    #[test]
    fn accepts_exactly_thirty_two_leaves_and_rejects_the_next() {
        let accepted = std::iter::repeat_n("runner", MAX_LEAF_OCCURRENCES)
            .collect::<Vec<_>>()
            .join(" && ");
        assert_eq!(
            expand_script_graph("test", &scripts(&[("test", &accepted)]))
                .unwrap()
                .leaves
                .len(),
            MAX_LEAF_OCCURRENCES
        );

        let rejected = format!("{accepted} && runner");
        assert_eq!(
            expand_script_graph("test", &scripts(&[("test", &rejected)])).unwrap_err(),
            Reason::ScriptGraphTooLarge
        );
    }

    #[test]
    fn rejects_missing_or_non_exact_script_references() {
        for command in [
            "node --run missing",
            "npm run missing",
            "pnpm run missing",
            "node --run child -- extra",
            "npm run child --flag",
            "pnpm run child -- --arg",
            "npm --workspace apps run child",
            "pnpm --filter pkg run child",
            "npm run 'child*'",
        ] {
            let map = scripts(&[("test", command), ("child", "runner")]);
            assert_eq!(
                expand_script_graph("test", &map).unwrap_err(),
                Reason::ScriptReferenceUnsupported,
                "reference should be rejected: {command}"
            );
        }
    }

    #[test]
    fn propagates_tokenizer_failures_without_retaining_script_text() {
        assert_eq!(
            expand_script_graph("test", &scripts(&[("test", "runner | secret")])).unwrap_err(),
            Reason::ScriptSyntaxUnsupported
        );
    }
}
