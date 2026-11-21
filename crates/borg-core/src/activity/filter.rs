//! What the processes' filter keeps: every word must match, either inside the process's name,
//! ignoring case, or, for a number, as the process's whole PID.

use super::ProcessInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    text: String,
    pid: Option<u32>,
}

impl Word {
    fn new(word: &str) -> Self {
        let text = word.to_lowercase();
        let pid = text.parse::<u32>().ok().filter(|pid| pid.to_string() == text);
        Self { text, pid }
    }

    fn matches(&self, process: &ProcessInfo, name: &str) -> bool {
        self.pid == Some(process.key.pid) || name.contains(&self.text)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcessFilter {
    words: Vec<Word>,
}

impl ProcessFilter {
    pub(crate) fn parse(text: &str) -> Self {
        Self { words: text.split_whitespace().map(Word::new).collect() }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub(crate) fn keeps(&self, process: &ProcessInfo) -> bool {
        let name = process.name.to_lowercase();
        self.words.iter().all(|word| word.matches(process, &name))
    }

    /// The PIDs the filter's numbers name, which the cursor goes to as they are typed.
    pub(crate) fn pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.words.iter().filter_map(|word| word.pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::ProcessKey;

    fn process(pid: u32, name: &str) -> ProcessInfo {
        ProcessInfo {
            key: ProcessKey { pid, start: 1 },
            parent: None,
            name: name.into(),
            executable: None,
            cpu: None,
            resident: None,
            virtual_bytes: None,
            read: None,
            written: None,
            protection: None,
            nice: None,
            stopped: false,
        }
    }

    fn kept<'a>(filter: &str, processes: &'a [ProcessInfo]) -> Vec<&'a str> {
        let filter = ProcessFilter::parse(filter);
        processes.iter().filter(|process| filter.keeps(process)).map(|process| process.name.as_str()).collect()
    }

    #[test]
    fn a_number_keeps_the_process_with_that_whole_pid_and_names_that_contain_it() {
        let processes = [process(4100, "postgres"), process(41, "launchd"), process(3415, "node"), process(7, "x41")];
        assert_eq!(kept("4100", &processes), ["postgres"]);
        assert_eq!(kept("41", &processes), ["launchd", "x41"], "part of a PID matches no other PID");
        assert_eq!(kept("041", &processes), Vec::<&str>::new(), "a PID is written without leading zeros");
        assert_eq!(ProcessFilter::parse("node 4100 41").pids().collect::<Vec<_>>(), [4100, 41]);
    }

    #[test]
    fn every_word_must_match_the_name_ignoring_case_or_be_the_pid() {
        let processes = [process(4100, "Google Chrome Helper"), process(4200, "node"), process(4300, "node")];
        assert_eq!(kept("chrome HELPER", &processes), ["Google Chrome Helper"]);
        assert_eq!(kept("node 4300", &processes), ["node"]);
        assert_eq!(kept("node 4100", &processes), Vec::<&str>::new());
        assert_eq!(kept("  ", &processes).len(), 3, "blanks keep everything");
        assert!(ProcessFilter::parse(" ").is_empty());
    }
}
