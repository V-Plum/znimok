//! Ready scenarios (MCP prompts, ZK-236): what a person usually wants from Znimok through an
//! agent, written once — the client offers them as commands. Each names the tools in the order
//! they are used; the test keeps those names true.

use serde_json::{Value, json};

struct Prompt {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// (name, description, required)
    args: &'static [(&'static str, &'static str, bool)],
    /// The text; `{arg}` is replaced by the argument (or by a neutral phrase when it is absent).
    text: &'static str,
}

const PROMPTS: &[Prompt] = &[
    Prompt {
        name: "bug_report",
        title: "Bug report with a screenshot",
        description: "Capture the problem, cover what is private, mark it up and hand over a picture for the issue.",
        args: &[
            ("problem", "What went wrong, in a sentence", true),
            (
                "window",
                "A word from the title of the window that shows it",
                false,
            ),
        ],
        text: "Make a bug report screenshot with Znimok. The problem: {problem}.\n\
1. `list_targets`, pick the window{window}; `capture` it (target window; a region if only a part matters).\n\
2. `redact_pii` — cover keys, e-mails, cards and faces; tell me what was covered.\n\
3. `ocr` with `find` to locate the words of the error; `marks` (add): a frame around the problem, an arrow to it, a short text saying what is wrong. Keep it to three or four marks.\n\
4. `set_meta`: a name that states the problem, the tag «bug», a description with the steps you can see.\n\
5. `export` as `png` and give me the path. Answer in my language.",
    },
    Prompt {
        name: "document_screen",
        title: "Document a screen",
        description: "A numbered walkthrough of a window: counters next to the controls and a legend.",
        args: &[("app", "The app or window to document", true)],
        text: "Document the screen of {app} with Znimok.\n\
1. `list_targets` → `capture` (target window).\n\
2. `ocr` to read the labels and get their boxes.\n\
3. `marks` (add): a `counter` next to every control worth explaining (in reading order), no more than nine; a thin frame where a group needs one.\n\
4. `set_meta`: a description that lists the numbers with one line each — what the control does.\n\
5. `export` as `html` (the page carries the list of marks) and give me the path. Answer in my language.",
    },
    Prompt {
        name: "redact_before_sharing",
        title: "Hide secrets before sharing",
        description: "Find and cover keys, passwords, e-mails, phones, cards and faces on a screenshot.",
        args: &[(
            "document",
            "The document's id or name; the newest one when left out",
            false,
        )],
        text: "Prepare a Znimok screenshot for sharing{document}.\n\
1. `library_search` to find it (the newest when I named none).\n\
2. `redact_pii` with `apply: false` — tell me what would be hidden.\n\
3. `redact_pii` to apply; if something private is still readable, cover it with `marks` (add a `hide`, mode `plate`).\n\
4. `export` as `png` and give me the path. Answer in my language.",
    },
    Prompt {
        name: "read_recording",
        title: "Read a recording with the DevTools log",
        description: "What went wrong in a screen recording: errors, failed requests and clicks, with their times.",
        args: &[(
            "document",
            "The recording's id or name; the newest one with a log when left out",
            false,
        )],
        text: "Read a Znimok recording and tell me what went wrong{document}.\n\
1. `library_search` with `has_log: true` (and `kind: video`) to find it.\n\
2. `video_info` — its length, the clicks, the marks.\n\
3. `devlog` (summary) — the errors, the failed requests, the pages, each with its time in the video.\n\
4. `devlog` with `index` for each error or failed request that matters: the stack, the status, the response.\n\
5. Tell me, in order of time: what the person did (pages, clicks), what failed and why you think so, and at which second to look. Offer to put that into the description with `set_meta`. Answer in my language.",
    },
];

/// `prompts/list`.
pub fn list() -> Vec<Value> {
    PROMPTS
        .iter()
        .map(|p| {
            json!({
                "name": p.name, "title": p.title, "description": p.description,
                "arguments": p.args.iter().map(|(n, d, r)| json!({"name": n, "description": d, "required": r})).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// `prompts/get`: the scenario with the arguments put in.
pub fn get(name: &str, args: Option<&Value>) -> Option<Value> {
    let p = PROMPTS.iter().find(|p| p.name == name)?;
    let mut text = p.text.to_string();
    for (arg, _, required) in p.args {
        let given = args
            .and_then(|a| a.get(*arg))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let put = match (given, *arg, *required) {
            (Some(v), "window", _) => format!(" whose title has «{v}»"),
            (Some(v), "document", _) => format!(": «{v}»"),
            (Some(v), _, _) => v.to_string(),
            (None, _, true) => "(ask me)".to_string(),
            (None, _, false) => String::new(),
        };
        text = text.replace(&format!("{{{arg}}}"), &put);
    }
    Some(json!({
        "description": p.description,
        "messages": [{"role": "user", "content": {"type": "text", "text": text}}],
    }))
}

/// Every tool a scenario names (in backticks), for the test.
#[cfg(test)]
pub(crate) fn named_tools() -> Vec<String> {
    let mut out = Vec::new();
    for p in PROMPTS {
        for (i, part) in p.text.split('`').enumerate() {
            if i % 2 == 1
                && part.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                && part.contains('_')
            {
                out.push(part.to_string());
            }
        }
    }
    out
}
