//! Processes as a tree: the list's rows with the branches folded away, and the selected
//! process's family from the first process down to everything it started.

use std::collections::{HashMap, HashSet};

use super::{Activity, ProcessInfo, ProcessKey};

/// A process in the list, how deep in the tree it sits, and what happened to the processes it
/// started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListedProcess {
    /// Where the process is in the sample.
    pub index: usize,
    pub depth: usize,
    pub branch: Branch,
}

/// The processes a row started, as the tree shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    /// It started none that are listed, or the list is flat.
    Leaf,
    /// They are listed under it.
    Open,
    /// They are folded away under it: this many processes in all, theirs included.
    Folded(usize),
}

/// A process in the selected one's family and how it is related to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relative {
    /// Where the process is in the sample.
    pub index: usize,
    pub depth: usize,
    pub kin: Kin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kin {
    /// A process the selected one descends from.
    Ancestor,
    Selected,
    /// A process the selected one started, or one of theirs.
    Descendant,
}

/// The processes each one started, by the starting one's PID, in the order given.
fn children(processes: &[ProcessInfo], order: impl IntoIterator<Item = usize>) -> HashMap<u32, Vec<usize>> {
    let pids: HashSet<u32> = processes.iter().map(|process| process.key.pid).collect();
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for index in order {
        let process = &processes[index];
        if let Some(parent) = process.parent.filter(|pid| pids.contains(pid) && *pid != process.key.pid) {
            children.entry(parent).or_default().push(index);
        }
    }
    children
}

/// The listed processes as a tree in the order given: each one under the process that started
/// it, a folded one with none of its processes under it.
pub(super) fn tree(processes: &[ProcessInfo], order: &[usize], folded: &HashSet<ProcessKey>) -> Vec<ListedProcess> {
    let listed: HashSet<u32> = order.iter().map(|&index| processes[index].key.pid).collect();
    let children = children(processes, order.iter().copied());
    let roots = order.iter().copied().filter(|&index| {
        let process = &processes[index];
        process.parent.is_none_or(|parent| !listed.contains(&parent) || parent == process.key.pid)
    });
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for root in roots.collect::<Vec<_>>().into_iter().chain(order.iter().copied()) {
        let mut stack = vec![(root, 0)];
        while let Some((index, depth)) = stack.pop() {
            if !seen.insert(index) {
                continue;
            }
            let process = &processes[index];
            let started = children.get(&process.key.pid).map_or(&[][..], Vec::as_slice);
            let branch = if started.is_empty() {
                Branch::Leaf
            } else if folded.contains(&process.key) {
                Branch::Folded(fold_away(processes, started, &children, &mut seen))
            } else {
                stack.extend(started.iter().rev().map(|&child| (child, depth + 1)));
                Branch::Open
            };
            rows.push(ListedProcess { index, depth, branch });
        }
    }
    rows
}

/// Marks a folded row's processes and all of theirs as placed, and counts them.
fn fold_away(
    processes: &[ProcessInfo],
    started: &[usize],
    children: &HashMap<u32, Vec<usize>>,
    seen: &mut HashSet<usize>,
) -> usize {
    let mut stack = started.to_vec();
    let mut hidden = 0;
    while let Some(index) = stack.pop() {
        if seen.insert(index) {
            hidden += 1;
            stack.extend(children.get(&processes[index].key.pid).into_iter().flatten().copied());
        }
    }
    hidden
}

impl Activity {
    /// The selected process's family: the processes it descends from, the first one on top,
    /// then the process itself and everything it started, each under the one that started it,
    /// in PID order.
    pub fn family(&self) -> Vec<Relative> {
        let processes = &self.sample.processes;
        let Some(selected) = processes.iter().position(|process| Some(process.key) == self.selected) else {
            return Vec::new();
        };
        let by_pid: HashMap<u32, usize> =
            processes.iter().enumerate().map(|(index, process)| (process.key.pid, index)).collect();
        let mut line = vec![selected];
        let mut visited = HashSet::from([selected]);
        let mut current = selected;
        while let Some(parent) =
            processes[current].parent.and_then(|pid| by_pid.get(&pid).copied()).filter(|index| visited.insert(*index))
        {
            line.push(parent);
            current = parent;
        }
        line.reverse();
        let ancestors = line.len() - 1;
        let mut family: Vec<Relative> = line
            .into_iter()
            .enumerate()
            .map(|(depth, index)| Relative {
                index,
                depth,
                kin: if depth == ancestors { Kin::Selected } else { Kin::Ancestor },
            })
            .collect();
        let mut order: Vec<usize> = (0..processes.len()).collect();
        order.sort_by_key(|&index| processes[index].key.pid);
        let children = children(processes, order);
        let mut stack: Vec<(usize, usize)> = children
            .get(&processes[selected].key.pid)
            .into_iter()
            .flatten()
            .rev()
            .map(|&child| (child, ancestors + 1))
            .collect();
        while let Some((index, depth)) = stack.pop() {
            if !visited.insert(index) {
                continue;
            }
            family.push(Relative { index, depth, kin: Kin::Descendant });
            stack.extend(
                children.get(&processes[index].key.pid).into_iter().flatten().rev().map(|&child| (child, depth + 1)),
            );
        }
        family
    }

    /// Right unfolds the selected process, or else steps to the first process it started. Left
    /// folds it, or else folds the process that started it and selects that one. Only the tree
    /// folds.
    pub(crate) fn fold(&mut self, open: bool) {
        if !self.tree || !self.filter.is_empty() {
            return;
        }
        let Some(row) = self.rows.get(self.cursor).copied() else { return };
        let key = self.sample.processes[row.index].key;
        let target = match (open, row.branch) {
            (true, Branch::Folded(_)) => {
                self.folded.remove(&key);
                self.cursor
            }
            (true, Branch::Open) => self.cursor + 1,
            (true, Branch::Leaf) => return,
            (false, Branch::Open) => {
                self.folded.insert(key);
                self.cursor
            }
            (false, Branch::Leaf | Branch::Folded(_)) => {
                let Some(parent) = self.rows[..self.cursor].iter().rposition(|above| above.depth < row.depth) else {
                    return;
                };
                self.folded.insert(self.sample.processes[self.rows[parent].index].key);
                parent
            }
        };
        self.rebuild();
        self.cursor = target.min(self.rows.len().saturating_sub(1));
        self.select();
    }

    /// Forgets the folds of processes that have ended.
    pub(super) fn forget_ended_folds(&mut self) {
        let running: HashSet<ProcessKey> = self.sample.processes.iter().map(|process| process.key).collect();
        self.folded.retain(|key| running.contains(key));
    }
}
