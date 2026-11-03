//! What each place's files say starts: the program, its arguments, and when it runs.

use super::{StartupFormat, plist};

/// One thing a file starts, before the app looks at the file and its program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupEntry {
    /// The line it is on, counted from 1, for files with one item per line.
    pub line: Option<usize>,
    pub name: String,
    /// What it runs, as the file says it.
    pub runs: String,
    /// The program that runs: a path, or a name the system looks up.
    pub program: Option<String>,
    /// When it runs, in the file's own words.
    pub when: Option<String>,
    /// The user it runs as, when the file says.
    pub user: Option<String>,
    /// Turned off in the file itself.
    pub disabled: bool,
}

/// What a file at `name` in `format` starts.
pub fn parse(format: StartupFormat, name: &str, bytes: &[u8]) -> Vec<StartupEntry> {
    let text = String::from_utf8_lossy(bytes);
    match format {
        StartupFormat::Launchd => launchd(name, bytes).into_iter().collect(),
        StartupFormat::Crontab => crontab(&text, false),
        StartupFormat::SystemCrontab => crontab(&text, true),
        StartupFormat::Anacrontab => anacrontab(&text),
        StartupFormat::Unit => unit(name, &text).into_iter().collect(),
        StartupFormat::Desktop => desktop(name, &text).into_iter().collect(),
        StartupFormat::Script => vec![script(name, &text)],
        StartupFormat::Preload => preload(&text),
        StartupFormat::Shell => vec![shell(name, &text)],
        StartupFormat::AuthorizedKeys => authorized_keys(&text),
    }
}

const LAUNCHD_TRIGGERS: &[&str] = &[
    "RunAtLoad",
    "KeepAlive",
    "StartInterval",
    "StartCalendarInterval",
    "WatchPaths",
    "QueueDirectories",
    "StartOnMount",
];

fn launchd(name: &str, bytes: &[u8]) -> Option<StartupEntry> {
    let plist = plist::parse(bytes)?;
    let label = plist.get("Label").and_then(plist::Value::text).map_or_else(|| stem(name), str::to_owned);
    let arguments: Vec<String> = match plist.get("ProgramArguments") {
        Some(plist::Value::Array(items)) => items.iter().filter_map(plist::Value::text).map(str::to_owned).collect(),
        _ => Vec::new(),
    };
    let program = ["Program", "BundleProgram"]
        .iter()
        .find_map(|key| plist.get(key).and_then(plist::Value::text))
        .map(str::to_owned)
        .or_else(|| arguments.first().cloned());
    let shown: Vec<String> = match (&program, plist.get("Program")) {
        (Some(program), Some(_)) => std::iter::once(program.clone()).chain(arguments.iter().skip(1).cloned()).collect(),
        _ if arguments.is_empty() => program.iter().cloned().collect(),
        _ => arguments,
    };
    let when: Vec<String> = LAUNCHD_TRIGGERS
        .iter()
        .filter_map(|key| {
            let value = plist.get(key)?;
            match value {
                plist::Value::Bool(false) => None,
                plist::Value::Integer(number) => Some(format!("{key} {number}")),
                _ => Some((*key).to_owned()),
            }
        })
        .collect();
    Some(StartupEntry {
        line: None,
        name: label,
        runs: join(&shown),
        program,
        when: (!when.is_empty()).then(|| when.join(", ")),
        user: plist.get("UserName").and_then(plist::Value::text).map(str::to_owned),
        disabled: plist.get("Disabled").and_then(plist::Value::flag).unwrap_or(false),
    })
}

/// Arguments joined into one command line, each quoted as a shell would need it.
fn join(arguments: &[String]) -> String {
    arguments.iter().map(|argument| crate::shell_quote(argument)).collect::<Vec<_>>().join(" ")
}

fn stem(name: &str) -> String {
    std::path::Path::new(name).file_stem().map_or_else(|| name.to_owned(), |stem| stem.to_string_lossy().into_owned())
}

/// The lines that say something: numbered from 1, trimmed, without blank lines and comments.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
}

const CRON_FIELDS: usize = 5;

fn crontab(text: &str, with_user: bool) -> Vec<StartupEntry> {
    lines(text)
        .filter(|(_, line)| !assignment(line))
        .filter_map(|(number, line)| {
            let mut rest = line;
            let fields = if line.starts_with('@') { 1 } else { CRON_FIELDS };
            let mut schedule = Vec::with_capacity(fields);
            for _ in 0..fields {
                let (field, after) = first_word(rest)?;
                schedule.push(field);
                rest = after;
            }
            let user = if with_user {
                let (user, after) = first_word(rest)?;
                rest = after;
                Some(user.to_owned())
            } else {
                None
            };
            let command = rest.trim();
            (!command.is_empty()).then(|| StartupEntry {
                line: Some(number),
                name: program_of(command).map_or_else(|| command.to_owned(), |program| base_name(&program)),
                runs: command.to_owned(),
                program: program_of(command),
                when: Some(schedule.join(" ")),
                user,
                disabled: false,
            })
        })
        .collect()
}

/// An anacrontab line: the period in days, the delay in minutes, the job's name and the command.
fn anacrontab(text: &str) -> Vec<StartupEntry> {
    lines(text)
        .filter(|(_, line)| !assignment(line))
        .filter_map(|(number, line)| {
            let (period, rest) = first_word(line)?;
            let (_delay, rest) = first_word(rest)?;
            let (job, rest) = first_word(rest)?;
            let command = rest.trim();
            (!command.is_empty()).then(|| StartupEntry {
                line: Some(number),
                name: job.to_owned(),
                runs: command.to_owned(),
                program: program_of(command),
                when: Some(period.to_owned()),
                ..StartupEntry::default()
            })
        })
        .collect()
}

/// Whether a crontab line sets a variable rather than scheduling a command.
fn assignment(line: &str) -> bool {
    let Some((name, _)) = line.split_once('=') else { return false };
    let name = name.trim_end();
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn first_word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start();
    if text.is_empty() {
        return None;
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    Some((&text[..end], &text[end..]))
}

fn base_name(program: &str) -> String {
    program.rsplit('/').next().unwrap_or(program).to_owned()
}

const UNIT_TIMES: &[&str] =
    &["OnCalendar", "OnBootSec", "OnStartupSec", "OnActiveSec", "OnUnitActiveSec", "OnUnitInactiveSec"];
const SERVICE_SUFFIX: &str = ".service";
const TIMER_SUFFIX: &str = ".timer";
/// Characters before an `ExecStart` program that change how systemd runs it.
const EXEC_PREFIXES: &[char] = &['@', '-', ':', '+', '!'];

/// A systemd unit: the program a service starts, or the service a timer starts and when.
fn unit(name: &str, text: &str) -> Option<StartupEntry> {
    let mut section = "";
    let mut start: Option<(usize, String)> = None;
    let mut times = Vec::new();
    let mut target = None;
    let mut wanted_by = None;
    let mut user = None;
    let joined = continued(text);
    for (number, line) in &joined {
        if let Some(header) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            section = header;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        match (section, key) {
            ("Service", "ExecStart") if value.is_empty() => start = None,
            ("Service", "ExecStart") if start.is_none() => {
                start = Some((*number, value.trim_start_matches(EXEC_PREFIXES).to_owned()));
            }
            ("Service", "User") => user = Some(value.to_owned()),
            ("Timer", "Unit") => target = Some(value.to_owned()),
            ("Timer", key) if UNIT_TIMES.contains(&key) => times.push(format!("{key}={value}")),
            ("Install", "WantedBy" | "RequiredBy") => wanted_by = Some(format!("{key}={value}")),
            _ => {}
        }
    }
    if let Some(timer) = name.strip_suffix(TIMER_SUFFIX) {
        let service = target.unwrap_or_else(|| format!("{timer}{SERVICE_SUFFIX}"));
        return Some(StartupEntry {
            name: name.to_owned(),
            runs: service,
            when: (!times.is_empty()).then(|| times.join(", ")),
            ..StartupEntry::default()
        });
    }
    let (line, command) = start?;
    Some(StartupEntry {
        line: Some(line),
        name: name.to_owned(),
        program: program_of(&command),
        runs: command,
        when: wanted_by,
        user,
        disabled: false,
    })
}

/// A unit's lines with those ending in a backslash joined to the next, each numbered by the
/// line it starts on, without blank lines and comments.
fn continued(text: &str) -> Vec<(usize, String)> {
    let mut joined: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let (body, more) = match trimmed.strip_suffix('\\') {
            Some(body) => (body.trim_end(), true),
            None => (trimmed, false),
        };
        match joined.last_mut().filter(|_| open) {
            Some((_, text)) => {
                text.push(' ');
                text.push_str(body);
            }
            None if body.is_empty() || body.starts_with(['#', ';']) => {}
            None => joined.push((index + 1, body.to_owned())),
        }
        open = more;
    }
    joined
}

const DESKTOP_SECTION: &str = "[Desktop Entry]";
/// Field codes an `Exec` line may hold, which the desktop fills in.
const FIELD_CODES: &[&str] = &["%f", "%F", "%u", "%U", "%d", "%D", "%n", "%N", "%i", "%c", "%k", "%v", "%m"];

/// An XDG autostart entry: what its `Exec` runs, unless `Hidden` or GNOME's switch turns it off.
fn desktop(name: &str, text: &str) -> Option<StartupEntry> {
    let mut inside = false;
    let mut found = StartupEntry { name: stem(name), ..StartupEntry::default() };
    let mut exec = None;
    for (number, line) in lines(text) {
        if line.starts_with('[') {
            inside = line == DESKTOP_SECTION;
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| inside) else { continue };
        let value = value.trim();
        match key.trim() {
            "Name" => found.name = value.to_owned(),
            "Exec" => exec = Some((number, value)),
            "Hidden" => found.disabled |= value.eq_ignore_ascii_case("true"),
            "X-GNOME-Autostart-enabled" => found.disabled |= value.eq_ignore_ascii_case("false"),
            _ => {}
        }
    }
    let (line, exec) = exec?;
    let command = exec.split_whitespace().filter(|word| !FIELD_CODES.contains(word)).collect::<Vec<_>>().join(" ");
    let command = command.replace("%%", "%");
    Some(StartupEntry { line: Some(line), program: program_of(&command), runs: command, ..found })
}

/// Separates the commands a script shows in one line.
pub const COMMAND_SEPARATOR: &str = "; ";
const END_OF_SCRIPT: &str = "exit 0";

/// A script that runs as a whole: its commands in one line, and the first one's program.
fn script(name: &str, text: &str) -> StartupEntry {
    let commands: Vec<(usize, &str)> = lines(text).filter(|(_, line)| *line != END_OF_SCRIPT).collect();
    StartupEntry {
        line: None,
        name: name.to_owned(),
        runs: commands.iter().map(|(_, command)| *command).collect::<Vec<_>>().join(COMMAND_SEPARATOR),
        program: commands.iter().find_map(|(_, command)| program_of(command)),
        ..StartupEntry::default()
    }
}

/// A shell's startup file: everything in it runs in every shell that reads it, so it is one item.
fn shell(name: &str, text: &str) -> StartupEntry {
    StartupEntry {
        name: name.to_owned(),
        runs: lines(text).map(|(_, line)| line).collect::<Vec<_>>().join(COMMAND_SEPARATOR),
        ..StartupEntry::default()
    }
}

/// `/etc/ld.so.preload`: each library it names, which the loader puts into every program.
fn preload(text: &str) -> Vec<StartupEntry> {
    lines(text)
        .flat_map(|(number, line)| {
            line.split([' ', '\t', ':']).filter(|library| !library.is_empty()).map(move |library| StartupEntry {
                line: Some(number),
                name: base_name(library),
                runs: library.to_owned(),
                program: Some(library.to_owned()),
                ..StartupEntry::default()
            })
        })
        .collect()
}

const KEY_TYPE_PREFIXES: &[&str] = &["ssh-", "ecdsa-", "sk-ssh-", "sk-ecdsa-"];
const COMMAND_OPTION: &str = "command=";
const ELIDED_KEY: &str = "…";

/// Each key in an `authorized_keys` file: who it lets in, and the command it forces, if any.
fn authorized_keys(text: &str) -> Vec<StartupEntry> {
    lines(text)
        .filter_map(|(number, line)| {
            let words = key_words(line);
            let typed =
                words.iter().position(|word| KEY_TYPE_PREFIXES.iter().any(|prefix| word.starts_with(prefix)))?;
            let options = (typed > 0).then(|| words[..typed].join(" "));
            let kind = &words[typed];
            let comment = words.get(typed + 2..).map(|rest| rest.join(" ")).filter(|comment| !comment.is_empty());
            let command = options.as_deref().and_then(forced_command);
            let shown = [options.clone(), Some(kind.clone()), Some(ELIDED_KEY.to_owned()), comment.clone()];
            Some(StartupEntry {
                line: Some(number),
                name: comment.unwrap_or_else(|| kind.clone()),
                runs: shown.into_iter().flatten().collect::<Vec<_>>().join(" "),
                program: command.as_deref().and_then(program_of),
                ..StartupEntry::default()
            })
        })
        .collect()
}

/// A key line's words: options with quoted commas and spaces kept whole, the key type, the key
/// and the comment.
fn key_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in line.chars() {
        match c {
            _ if escaped => {
                word.push(c);
                escaped = false;
            }
            '\\' if quoted => {
                word.push(c);
                escaped = true;
            }
            '"' => {
                word.push(c);
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The command a key's `command="…"` option forces, without its quotes.
fn forced_command(options: &str) -> Option<String> {
    let start = options.find(COMMAND_OPTION)? + COMMAND_OPTION.len();
    let quoted = options[start..].strip_prefix('"')?;
    let mut command = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => command.extend(chars.next()),
            '"' => return Some(command),
            c => command.push(c),
        }
    }
    None
}

/// Words a command starts with that run the word after them.
const WRAPPERS: &[&str] = &["exec", "nohup", "env", "command", "builtin", "time"];
const OPERATORS: &[char] = &[';', '|', '&', '<', '>', '(', ')', '`'];

/// The program a shell command line runs: its first word after variable assignments and words
/// such as `exec`, `nohup` or `env`, with their options. `None` when a variable or a command
/// substitution names it.
pub fn program_of(command: &str) -> Option<String> {
    let mut wrapped = false;
    for word in shell_words(command) {
        if assignment_word(&word) || wrapped && word.starts_with('-') {
            continue;
        }
        if WRAPPERS.contains(&base_name(&word).as_str()) {
            wrapped = true;
            continue;
        }
        if word.contains('$') {
            return None;
        }
        return Some(word);
    }
    None
}

fn assignment_word(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The words of a command line up to its first operator, with quotes and backslashes resolved.
pub fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                word.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                started = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => word.extend(chars.next()),
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                started = true;
                word.extend(chars.next());
            }
            c if OPERATORS.contains(&c) => break,
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}
