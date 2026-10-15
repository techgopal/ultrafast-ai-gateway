//! Finding a tool among many: a lexical index (BM25) over the tools of the
//! registry, and the two tools the combined endpoint offers in place of a
//! long list, `find_tools` and `describe_tools`.
//!
//! The index is built with each snapshot. It is deterministic and holds no
//! embeddings. A search is scored over the tools the caller may call only, so
//! what the caller may not see cannot move a score, let alone appear.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::snapshot::SnapMcpTool;

/// BM25 parameters.
const K1: f64 = 1.2;
const B: f64 = 0.75;
/// What a search returns at most.
pub const MAX_RESULTS: usize = 20;
/// The most names `describe_tools` takes.
pub const MAX_DESCRIBE: usize = 20;
/// Added to a tool whose name is the query: an exact name comes first.
const EXACT_BONUS: f64 = 1000.0;

/// Lower case words: runs of letters and digits.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

struct Doc {
    server: String,
    tool: String,
    name_tokens: Vec<String>,
    terms: HashMap<String, u32>,
    len: u32,
}

/// The tools of a snapshot, ready to be searched.
#[derive(Default)]
pub struct ToolIndex {
    docs: Vec<Doc>,
}

/// One result: the server and the tool, and the score.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit<'a> {
    pub server: &'a str,
    pub tool: &'a str,
    pub score: f64,
}

impl ToolIndex {
    /// `tools` with the name of the server of each.
    pub fn build<'a>(tools: impl Iterator<Item = &'a SnapMcpTool>) -> Self {
        let docs = tools
            .map(|t| {
                let name_tokens = tokenize(&t.name);
                let mut terms: HashMap<String, u32> = HashMap::new();
                let mut len = 0;
                // The name counts three times, the title twice: what a tool
                // is called says most about it.
                let mut add = |words: Vec<String>, times: u32| {
                    for w in words {
                        *terms.entry(w).or_default() += times;
                        len += times;
                    }
                };
                add(name_tokens.clone(), 3);
                add(tokenize(t.title.as_deref().unwrap_or_default()), 2);
                add(tokenize(&t.description), 1);
                add(tokenize(&t.server_name), 1);
                Doc {
                    server: t.server_name.clone(),
                    tool: t.name.clone(),
                    name_tokens,
                    terms,
                    len,
                }
            })
            .collect();
        Self { docs }
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The best `limit` tools for `query` among those `visible` says yes to,
    /// best first; equal scores by name. Nothing matches an empty query.
    pub fn search<'a>(
        &'a self,
        query: &str,
        limit: usize,
        visible: &dyn Fn(&str, &str) -> bool,
    ) -> Vec<Hit<'a>> {
        let terms = {
            let mut seen = tokenize(query);
            seen.sort();
            seen.dedup();
            seen
        };
        if terms.is_empty() {
            return Vec::new();
        }
        let docs: Vec<&Doc> = self
            .docs
            .iter()
            .filter(|d| visible(&d.server, &d.tool))
            .collect();
        if docs.is_empty() {
            return Vec::new();
        }
        let n = docs.len() as f64;
        let average = docs.iter().map(|d| f64::from(d.len)).sum::<f64>() / n;
        let idf: Vec<f64> = terms
            .iter()
            .map(|t| {
                let df = docs.iter().filter(|d| d.terms.contains_key(t)).count() as f64;
                ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
            })
            .collect();
        let wanted = tokenize(query);
        let mut hits: Vec<Hit<'a>> = docs
            .iter()
            .filter_map(|d| {
                let mut score = 0.0;
                for (t, idf) in terms.iter().zip(&idf) {
                    let tf = f64::from(d.terms.get(t).copied().unwrap_or(0));
                    if tf > 0.0 {
                        score += idf * tf * (K1 + 1.0)
                            / (tf + K1 * (1.0 - B + B * f64::from(d.len) / average));
                    }
                }
                if !wanted.is_empty() && wanted == d.name_tokens {
                    score += EXACT_BONUS;
                }
                (score > 0.0).then(|| Hit {
                    server: &d.server,
                    tool: &d.tool,
                    score,
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| (a.server, a.tool).cmp(&(b.server, b.tool)))
        });
        hits.truncate(limit);
        hits
    }
}

pub const FIND_TOOLS: &str = "find_tools";
pub const DESCRIBE_TOOLS: &str = "describe_tools";

/// Whether the name is one of the gateway's own tools.
pub fn is_meta(name: &str) -> bool {
    name == FIND_TOOLS || name == DESCRIBE_TOOLS
}

/// The two tools as `tools/list` shows them.
pub fn meta_tools() -> Vec<Value> {
    vec![
        json!({
            "name": FIND_TOOLS,
            "description": "Search the tools you may call by what you want to do. Returns the best matches, each with the name to call. Use describe_tools for the arguments of one.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What you want to do, in a few words."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS, "description": "How many matches, 5 by default."}
                },
                "required": ["query"]
            },
            "_meta": {"ultrafast": {"native": true}},
        }),
        json!({
            "name": DESCRIBE_TOOLS,
            "description": "The full description and input schema of tools found with find_tools.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "names": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_DESCRIBE,
                              "description": "Tool names as find_tools gave them (server__tool)."}
                },
                "required": ["names"]
            },
            "_meta": {"ultrafast": {"native": true}},
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(server: &str, name: &str, description: &str) -> SnapMcpTool {
        SnapMcpTool {
            id: 0,
            server_id: 0,
            server_name: server.into(),
            name: name.into(),
            title: None,
            description: description.into(),
            input_schema: json!({}),
            output_schema: None,
            annotations: None,
            everyone: false,
            team_ids: Vec::new(),
            user_ids: Vec::new(),
        }
    }

    fn index() -> ToolIndex {
        let tools = [
            tool("github", "create_issue", "Opens an issue in a repository"),
            tool("github", "delete_repo", "Deletes a repository for good"),
            tool("github", "search_code", "Searches code across repositories"),
            tool("docs", "search", "Searches the documentation pages"),
            tool("calendar", "create_event", "Adds an event to the calendar"),
        ];
        ToolIndex::build(tools.iter())
    }

    fn names(hits: &[Hit<'_>]) -> Vec<String> {
        hits.iter()
            .map(|h| format!("{}/{}", h.server, h.tool))
            .collect()
    }

    #[test]
    fn words_are_lower_case_runs_of_letters_and_digits() {
        assert_eq!(
            tokenize("Create_Issue v2-beta"),
            ["create", "issue", "v2", "beta"]
        );
        assert!(tokenize("  ").is_empty());
    }

    #[test]
    fn the_best_match_comes_first_and_an_exact_name_beats_the_rest() {
        let i = index();
        let all = |_: &str, _: &str| true;
        assert_eq!(
            names(&i.search("delete repo", 5, &all))[0],
            "github/delete_repo"
        );
        assert_eq!(
            names(&i.search("create issue", 5, &all))[0],
            "github/create_issue"
        );
        // The exact name of `docs/search` beats `github/search_code`, which also says search.
        assert_eq!(names(&i.search("search", 5, &all))[0], "docs/search");
        assert_eq!(i.search("zebra", 5, &all), []);
        assert_eq!(i.search("   ", 5, &all), []);
        assert_eq!(i.search("create", 1, &all).len(), 1);
    }

    #[test]
    fn what_is_not_visible_neither_matches_nor_moves_a_score() {
        let i = index();
        let only_calendar = |s: &str, _: &str| s == "calendar";
        assert_eq!(
            names(&i.search("delete repo", 5, &only_calendar)),
            Vec::<String>::new()
        );
        assert_eq!(
            names(&i.search("create", 5, &only_calendar)),
            ["calendar/create_event"]
        );
        // The score over the visible tools is the same however many hidden
        // ones the registry holds.
        let few = ToolIndex::build([tool("calendar", "create_event", "Adds an event")].iter());
        let many = ToolIndex::build(
            [
                tool("calendar", "create_event", "Adds an event"),
                tool("x", "create_thing", "create create create"),
                tool("y", "create_other", "create"),
            ]
            .iter(),
        );
        let only = |s: &str, _: &str| s == "calendar";
        let a = few.search("create event", 5, &only)[0].score;
        let b = many.search("create event", 5, &only)[0].score;
        assert!((a - b).abs() < 1e-9, "{a} {b}");
    }

    #[test]
    fn the_meta_tools_are_two_with_schemas() {
        let tools = meta_tools();
        assert_eq!(tools.len(), 2);
        assert!(is_meta("find_tools") && is_meta("describe_tools") && !is_meta("a__b"));
        assert_eq!(tools[0]["inputSchema"]["required"][0], "query");
    }
}
