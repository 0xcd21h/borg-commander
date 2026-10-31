use crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};

use crate::{AppState, Effect, Overlay};

pub const SIDE: usize = 4;
pub const CELLS: usize = SIDE * SIDE;
const FORMAT: u8 = 1;
const TARGET: u8 = 11;
const MAX_EXPONENT: u8 = 60;
const RANDOM_SEED: u64 = 0x9e37_79b9_7f4a_7c15;
const RANDOM_LEFT: u32 = 13;
const RANDOM_RIGHT: u32 = 7;
const RANDOM_LAST: u32 = 17;
const FOUR_CHANCE: u64 = 10;
const INITIAL_TILES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    Playing,
    Won,
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedGame2048 {
    pub version: u8,
    pub cells: [u8; CELLS],
    pub score: u64,
    pub best: u64,
    pub random: u64,
    pub continued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Position {
    cells: [u8; CELLS],
    score: u64,
    random: u64,
    continued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game2048 {
    position: Position,
    best: u64,
    undo: Option<Position>,
}

impl Game2048 {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            position: Position {
                cells: [0; CELLS],
                score: 0,
                random: if seed == 0 { RANDOM_SEED } else { seed },
                continued: false,
            },
            best: 0,
            undo: None,
        };
        for _ in 0..INITIAL_TILES {
            game.spawn();
        }
        game
    }

    pub fn restore(saved: SavedGame2048) -> Option<Self> {
        if saved.version != FORMAT
            || saved.cells.iter().any(|&cell| cell > MAX_EXPONENT)
            || saved.cells.iter().all(|&cell| cell == 0)
        {
            return None;
        }
        Some(Self {
            position: Position {
                cells: saved.cells,
                score: saved.score,
                random: if saved.random == 0 { RANDOM_SEED } else { saved.random },
                continued: saved.continued,
            },
            best: saved.best.max(saved.score),
            undo: None,
        })
    }

    pub fn saved(&self) -> SavedGame2048 {
        SavedGame2048 {
            version: FORMAT,
            cells: self.position.cells,
            score: self.position.score,
            best: self.best,
            random: self.position.random,
            continued: self.position.continued,
        }
    }

    pub fn cells(&self) -> &[u8; CELLS] {
        &self.position.cells
    }

    pub fn score(&self) -> u64 {
        self.position.score
    }

    pub fn best(&self) -> u64 {
        self.best
    }

    pub fn can_undo(&self) -> bool {
        self.undo.is_some()
    }

    pub fn status(&self) -> GameStatus {
        if !self.position.continued && self.cells().iter().any(|&cell| cell >= TARGET) {
            return GameStatus::Won;
        }
        let possible = self.cells().contains(&0)
            || (0..CELLS).any(|index| {
                let cell = self.cells()[index];
                cell < MAX_EXPONENT
                    && (index % SIDE + 1 < SIDE && cell == self.cells()[index + 1]
                        || index / SIDE + 1 < SIDE && cell == self.cells()[index + SIDE])
            });
        if possible { GameStatus::Playing } else { GameStatus::Over }
    }

    pub fn slide(&mut self, direction: GameDirection) -> bool {
        if self.status() != GameStatus::Playing {
            return false;
        }
        let mut cells = [0; CELLS];
        let mut gain: u64 = 0;
        for line in 0..SIDE {
            let values: Vec<_> =
                (0..SIDE).map(|offset| self.cells()[index(direction, line, offset)]).filter(|&cell| cell > 0).collect();
            let mut read = 0;
            let mut write = 0;
            while read < values.len() {
                let mut value = values[read];
                if value < MAX_EXPONENT && values.get(read + 1) == Some(&value) {
                    value += 1;
                    gain = gain.saturating_add(1u64 << value);
                    read += 1;
                }
                cells[index(direction, line, write)] = value;
                read += 1;
                write += 1;
            }
        }
        if cells == self.position.cells {
            return false;
        }
        self.undo = Some(self.position.clone());
        self.position.cells = cells;
        self.position.score = self.position.score.saturating_add(gain);
        self.best = self.best.max(self.position.score);
        self.spawn();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(position) = self.undo.take() else { return false };
        self.position = position;
        true
    }

    pub fn restart(&mut self) {
        let best = self.best;
        *self = Self::new(self.next_random());
        self.best = best;
    }

    pub fn keep_playing(&mut self) -> bool {
        if self.status() != GameStatus::Won {
            return false;
        }
        self.position.continued = true;
        true
    }

    fn next_random(&mut self) -> u64 {
        let mut value = self.position.random;
        value ^= value << RANDOM_LEFT;
        value ^= value >> RANDOM_RIGHT;
        value ^= value << RANDOM_LAST;
        self.position.random = value;
        value
    }

    fn spawn(&mut self) {
        let empty: Vec<_> = (0..CELLS).filter(|&index| self.cells()[index] == 0).collect();
        if empty.is_empty() {
            return;
        }
        let index = empty[(self.next_random() % empty.len() as u64) as usize];
        self.position.cells[index] = if self.next_random().is_multiple_of(FOUR_CHANCE) { 2 } else { 1 };
    }
}

fn index(direction: GameDirection, line: usize, offset: usize) -> usize {
    match direction {
        GameDirection::Left => line * SIDE + offset,
        GameDirection::Right => line * SIDE + SIDE - 1 - offset,
        GameDirection::Up => offset * SIDE + line,
        GameDirection::Down => (SIDE - 1 - offset) * SIDE + line,
    }
}

pub(crate) fn open(state: &mut AppState, now: std::time::Instant) -> Vec<Effect> {
    if state.game2048.is_none() {
        let elapsed = now.saturating_duration_since(state.epoch).as_nanos() as u64;
        state.game2048 = Some(Game2048::new(elapsed ^ RANDOM_SEED));
    }
    state.overlay = Some(Overlay::Game2048);
    vec![Effect::SaveGame2048]
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    if matches!(key.code, KeyCode::Esc | KeyCode::F(10)) {
        state.overlay = None;
        return vec![Effect::SaveGame2048];
    }
    let Some(game) = state.game2048.as_mut() else { return Vec::new() };
    let changed = match key.code {
        KeyCode::Left => game.slide(GameDirection::Left),
        KeyCode::Right => game.slide(GameDirection::Right),
        KeyCode::Up => game.slide(GameDirection::Up),
        KeyCode::Down => game.slide(GameDirection::Down),
        KeyCode::Char('u' | 'U') => game.undo(),
        KeyCode::Char('r' | 'R') => {
            game.restart();
            true
        }
        KeyCode::Enter => game.keep_playing(),
        _ => false,
    };
    if changed { vec![Effect::SaveGame2048] } else { Vec::new() }
}

#[cfg(test)]
mod tests;
