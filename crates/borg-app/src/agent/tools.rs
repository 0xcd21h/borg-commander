//! The tools `borg mcp` offers, as MCP describes them.

use super::capture::SUMMARY_ROWS;
use borg_core::{HEADER_BYTES, LONGEST_CAPTURE, SHORTEST_CAPTURE};
use serde_json::{Value, json};

/// The tools `borg mcp` offers, as MCP describes them: few and general. The agent looks with
/// `screen` and `read` without asking, searches a process's memory and captures traffic once
/// the user lets it, puts things in front of the user with `show`, and changes anything only
/// through `write`, `edit` and `run`, which wait for the user's approval.
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
            "description": format!("Reads without asking the user. With `path` (a location; the active panel's folder when left out): a folder's entries, or a text file's lines numbered like `cat -n`: the first 2000, or `lines` such as \"120-180\", \"120-\" or \"-50\" (the last 50), or only those containing the text `find`. Quote lines without their numbers when you edit. With `source` \"processes\": the `limit` (default 40) busiest processes on the machine Borg runs on, by `sort` \"memory\" (default) or \"cpu\". With `source` \"containers\": the Docker containers, their state and Compose project. With `source` \"ports\", \"connections\" or \"interfaces\": what the network screen lists, measured over one second: the listening ports with who can reach each and the process behind it, the connections busiest first with their rates and host names, or the interfaces with their addresses and traffic; `find` filters as the screen's filter does (words, port:443, proc:name, host:name, state:listen, !word) and `limit` caps the rows (default 100). With `source` \"startup\": what the startup items screen lists, newest change first: every launch agent and daemon, systemd unit and timer, cron and anacron job, periodic script, XDG autostart entry, rc.local, ld.so.preload library, shell startup file and authorized SSH key of the user Borg runs as, each with its file and line, owner, modification time, what it runs, whether its program is on disk, and the rules (Borg's and the user's YARA rules) the file or its program matched; `find` keeps items containing every word and none after `!`, and `limit` caps the items (default 100). Use it to see what was set to start recently. A capture file (.pcap, .pcapng or .cap) on the machine Borg runs on reads as the summary `capture` answers with, narrowed by `find` with the same filter words (proto:dns, host:name, pid:4100), and `limit` caps each list (default {SUMMARY_ROWS}); with `packet`, a packet's number as the summary's FIRST column or a packet's neighbours give it, it reads that packet's layers and its bytes in hex. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "lines": { "type": "string" },
                    "find": { "type": "string" },
                    "source": { "type": "string", "enum": ["processes", "containers", "ports", "connections", "interfaces", "startup"] },
                    "limit": { "type": "integer", "minimum": 1 },
                    "sort": { "type": "string", "enum": ["memory", "cpu"] },
                    "packet": { "type": "integer", "minimum": 1 }
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
            "name": "scan",
            "description": format!("Scans with YARA rules and checks code integrity on the machine Borg runs on, as Rule matches in Ctrl-F's Look for and Y and I in the memory map do. With `path` (a file, or a folder and every file below it on the same file system, hidden ones included), it runs the rules over each file up to {file_mib} MiB without asking, and stops after {files} files, {findings} matches or {seconds} seconds. With `pid` (get PIDs from read with source \"processes\"), it reads the memory the process filled itself after the user approves it in Borg, once per process: one approval covers scan and search_memory. `check` \"integrity\" compares the process's code with the files it was loaded from and looks for injected code and libraries; \"rules\" runs the rules; \"both\" is the default for a process. Each memory check stops after {seconds} seconds, {memory_gib} GiB of memory scanned or {findings} findings. The rules are Borg's own and the user's rules folder; `rules` replaces them with YARA rule text of your own, up to {inline_kib} KiB, compiled on its own with include turned off, so you can draft a rule from evidence, try it on a folder or a process, and refine it. Rule text that does not compile comes back as an error with its line and column. The answer lists findings worst first: severity, kind, the rule and where it comes from (borg, the path of the user's rule file, or inline), the file and offset or the address and memory region, up to {evidence} bytes of evidence as quoted text or hex, what the rule looks for and what else could explain it. Then come how much was read, what could not be read, whether the scan finished, and rule files left out because they do not compile. `limit` caps the findings listed (default 100). A finding is a lead to look into. Memory can hold passwords and keys: quote evidence only as far as the task needs. Where the system refuses to read the process, `admin` true asks the user for their password to read it as administrator. {LOCATIONS} Containers are not scanned.", file_mib = borg_core::CONTENTS_SIZE_LIMIT / (1024 * 1024), files = crate::rule_scan::FILE_LIMIT, findings = crate::rule_scan::FINDING_LIMIT, seconds = crate::rule_scan::TIME_LIMIT.as_secs(), memory_gib = crate::rule_scan::SCAN_LIMIT / (1024 * 1024 * 1024), inline_kib = borg_rules::INLINE_LIMIT / 1024, evidence = borg_rules::EVIDENCE_BYTES),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "pid": { "type": "integer", "minimum": 1 },
                    "check": { "type": "string", "enum": ["rules", "integrity", "both"] },
                    "rules": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 },
                    "admin": { "type": "boolean" }
                }
            },
            "annotations": read_only,
        },
        {
            "name": "capture",
            "description": format!("Captures traffic on the machine Borg runs on with the installed tcpdump, after the user approves it in Borg, as R on its network screen does, and answers with a summary: the protocols of the packets (DNS, TLS, HTTP and QUIC where Borg recognizes them), then each process, each host (named by the capture's DNS answers and the server names TLS and HTTP ask for) and each connection, with their packets and bytes. The summary leaves out what the packets carry. Without a scope it keeps everything on the interface; `port` keeps one port and the connections through it, over `transport` \"tcp\" (the default) or \"udp\"; `local` and `remote` (address:port, as read with source \"connections\" gives them) keep one connection both ways; `pid` captures everything and sums up that process's traffic. `interface` defaults to the one the network screen would pick: the one with the scope's address, else the busiest. `seconds` is 1 to {LONGEST_CAPTURE} (default {SHORTEST_CAPTURE}). `headers_only` keeps the first {HEADER_BYTES} bytes of each packet. `limit` caps each list (default {SUMMARY_ROWS}). The capture is saved as a .pcap file only the user can read, in `folder` (the active panel's folder when left out), and the answer says where: read that path to see the summary again, narrowed with `find`, or one packet with `packet`. Capturing needs tcpdump and the user's permission to open the capture device; without them the call fails and says so. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "interface": { "type": "string" },
                    "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
                    "transport": { "type": "string", "enum": ["tcp", "udp"] },
                    "local": { "type": "string" },
                    "remote": { "type": "string" },
                    "pid": { "type": "integer", "minimum": 1 },
                    "seconds": { "type": "integer", "minimum": 1, "maximum": LONGEST_CAPTURE },
                    "headers_only": { "type": "boolean" },
                    "folder": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1 }
                }
            },
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
            "description": "Copies a file from the machine Borg runs on into the Downloads folder of this computer, where you run. `path` is a location there; `to` optionally names a file or a folder inside ~/Downloads, and a folder keeps the file's name. A file already there is never replaced. The relay carries about 2 MB a second.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" }, "to": { "type": "string" } },
                "required": ["path"]
            },
        }),
        json!({
            "name": "upload",
            "description": "Copies a file from this computer, where you run, to `to`, a location on the machine Borg runs on, after the user approves it in Borg there, seeing which file it is. Up to 1 GiB; the relay carries about 2 MB a second.",
            "inputSchema": {
                "type": "object",
                "properties": { "from": { "type": "string" }, "to": { "type": "string" } },
                "required": ["from", "to"]
            },
            "annotations": { "destructiveHint": true },
        }),
    ]
}
