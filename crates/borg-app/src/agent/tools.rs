//! The tools `borg mcp` offers, as MCP describes them.

use serde_json::{Value, json};

/// The tools `borg mcp` offers, as MCP describes them: few and general. The agent looks with
/// `screen` and `read` without asking, searches a process's memory once the user lets it, puts
/// things in front of the user with `show`, and changes anything only through `write`, `edit`
/// and `run`, which wait for the user's approval.
pub(super) fn tools() -> Value {
    pub(super) const LOCATIONS: &str = "A location is an absolute path on the machine Borg runs on, ~/… for its home folder, docker://CONTAINER/path inside a container (by name, Compose service or id), or a path relative to the active panel's folder, wherever that panel is. `screen` names the panels this way.";
    let read_only = json!({ "readOnlyHint": true });
    json!([
        {
            "name": "screen",
            "description": "What the user sees in Borg right now: whether they look at the panels or the console, both panels' locations, the highlighted and marked files, where the command line runs and what is typed there, then the whole screen as text, as the user sees it (and the console's last lines while the panels hide it).",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only,
        },
        {
            "name": "read",
            "description": format!("Reads without asking the user. With `path` (a location; the active panel's folder when left out): a folder's entries, or a text file's lines numbered like `cat -n`: the first 2000, or `lines` such as \"120-180\", \"120-\" or \"-50\" (the last 50), or only those containing the text `find`. Quote lines without their numbers when you edit. With `source` \"processes\": the `limit` (default 40) busiest processes on the machine Borg runs on, by `sort` \"memory\" (default) or \"cpu\". With `source` \"containers\": the Docker containers, their state and Compose project. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "lines": { "type": "string" },
                    "find": { "type": "string" },
                    "source": { "type": "string", "enum": ["processes", "containers"] },
                    "limit": { "type": "integer", "minimum": 1 },
                    "sort": { "type": "string", "enum": ["memory", "cpu"] }
                }
            },
            "annotations": read_only,
        },
        {
            "name": "search_memory",
            "description": "Searches the memory of a running process on the machine Borg runs on, after the user approves it in Borg, once per process. Get PIDs from read with source \"processes\". `mode` \"indicators\" (the default) lists strings that look like URLs, IP addresses, email addresses, file paths, command lines, secrets or base64; \"strings\" lists every string that reads like text. Both look only at memory the process fills itself (heap, stacks, writable data), list each string once with how many copies memory holds, and compare it with the files mapped into the process: strings only memory holds come first, since the program built or received them while running (a decrypted configuration, a server it talks to, a command it was given). `query` keeps the strings that contain it. \"text\" finds where `query` lies in all readable memory, as UTF-8, UTF-16 or UTF-32, and answers with addresses only (to read the strings around it, search \"strings\" with that `query`), \"unicode\" also matches styled and combined letters, and \"hex\" finds bytes such as \"4D 5A 90 00\". `limit` caps the rows (default 100). A search reads for at most a minute. Memory can hold passwords and keys: quote them only as far as the task needs. Where the system refuses to read the process, `admin` true asks the user for their password to read it as administrator.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "minimum": 1 },
                    "mode": { "type": "string", "enum": ["indicators", "strings", "text", "unicode", "hex"] },
                    "query": { "type": "string" },
                    "match_case": { "type": "boolean" },
                    "limit": { "type": "integer", "minimum": 1 },
                    "admin": { "type": "boolean" }
                },
                "required": ["pid"]
            },
            "annotations": read_only,
        },
        {
            "name": "show",
            "description": format!("Shows a location to the user in Borg's other panel: a folder opens, a file is selected in its folder. The user's active panel stays where it is. Answers once the panel shows it. {LOCATIONS}"),
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] },
        },
        {
            "name": "write",
            "description": format!("Creates a text file, or replaces all of one, with `content`, after the user approves it in Borg: they see the new file, or the change as a diff. The folder must exist. For a small change to an existing file, use edit. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" }, "content": { "type": "string" } },
                "required": ["path", "content"]
            },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "edit",
            "description": format!("Replaces the text `old` in a file with `new`, after the user approves the diff in Borg. `old` must match the file exactly, spaces and line breaks included, and only once, unless `all` replaces every match; read the file first and include enough surrounding lines to make it unique. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old": { "type": "string" },
                    "new": { "type": "string" },
                    "all": { "type": "boolean" }
                },
                "required": ["path", "old", "new"]
            },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "run",
            "description": "Runs a shell command after the user approves it in Borg, and returns its exit status and output. It is typed into a shell the user watches: Borg's own command line on the machine Borg runs on, or with `at` set to docker://CONTAINER (or docker://CONTAINER/folder) that container's shell in Borg's Docker view. The shell keeps its folder and variables between commands; the user can answer prompts in it. `at` may also be a folder on the machine to run in. Explain what you run and why: the user sees each command before it runs. While the shell is busy (the user is typing, or a program runs), the command is refused; try again later.",
            "inputSchema": {
                "type": "object",
                "properties": { "command": { "type": "string" }, "at": { "type": "string" } },
                "required": ["command"]
            },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "type",
            "description": "Types `text` at the cursor of whatever runs in Borg's console (what Ctrl-O shows): the shell's command line, or an editor such as vim or nano. Nothing runs: Enter is never pressed, text with several lines goes in as a paste, and control keys are left out, so the user reviews the text, corrects it if needed, and runs or saves it. Use it to suggest a command the user runs, or to put a script into the editor the user has open. It is refused where the program there cannot take pasted text safely.",
            "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] },
        },
        {
            "name": "approval_mode",
            "description": "Requests Approve all for this paired computer in the selected Borg session, or turns it off. Enabling requires the user's approval in Borg. While enabled, this computer's commands, edits, and uploads run without separate prompts. It ends when this connection closes.",
            "inputSchema": { "type": "object", "properties": { "enabled": { "type": "boolean" } }, "required": ["enabled"] },
        }
    ])
}

pub(super) fn routing_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_sessions",
            "description": "Lists the local Borg and all paired remote targets with the IDs to use for subsequent calls.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "use_session",
            "description": "Selects the target for subsequent calls on this MCP connection. Other MCP connections are unaffected.",
            "inputSchema": {
                "type": "object",
                "properties": { "session": { "type": "string" } },
                "required": ["session"]
            }
        }),
        json!({
            "name": "list_remote_sessions",
            "description": "Lists named Borg sessions on one paired server and returns IDs for use_session.",
            "inputSchema": {
                "type": "object",
                "properties": { "server": { "type": "string" } },
                "required": ["server"]
            }
        }),
    ]
}

/// The tools only `borg mcp remote` offers: it runs on the user's computer, so it can copy
/// files between that and the server.
pub(super) fn transfer_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "download",
            "description": "Copies a file from the machine Borg runs on to this computer, where you run. `path` is a location there; `to` is a path here: a folder keeps the file's name, and ~/Downloads is used when it is left out. The relay carries about 2 MB a second.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" }, "to": { "type": "string" } },
                "required": ["path"]
            },
        }),
        json!({
            "name": "upload",
            "description": "Copies a file from this computer, where you run, to `to`, a location on the machine Borg runs on, after the user approves it in Borg there. Up to 1 GiB; the relay carries about 2 MB a second.",
            "inputSchema": {
                "type": "object",
                "properties": { "from": { "type": "string" }, "to": { "type": "string" } },
                "required": ["from", "to"]
            },
            "annotations": { "destructiveHint": true },
        }),
    ]
}
