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
mod tests {
    use super::*;

    fn board(cells: [u8; CELLS]) -> Game2048 {
        Game2048::restore(SavedGame2048 {
            version: FORMAT,
            cells,
            score: 0,
            best: 0,
            random: RANDOM_SEED,
            continued: false,
        })
        .unwrap()
    }

    fn mass(game: &Game2048) -> u64 {
        game.cells().iter().filter(|&&cell| cell > 0).map(|&cell| 1u64 << cell).sum()
    }

    #[test]
    fn merges_each_tile_once_and_scores_the_created_tiles() {
        for (row, expected, score) in [([1, 1, 1, 1], [2, 2], 8), ([1, 1, 2, 0], [2, 2], 4), ([2, 1, 1, 0], [2, 2], 4)]
        {
            let mut cells = [0; CELLS];
            cells[..SIDE].copy_from_slice(&row);
            let mut game = board(cells);
            let before = mass(&game);
            assert!(game.slide(GameDirection::Left));
            assert_eq!(&game.cells()[..2], &expected);
            assert_eq!(game.score(), score);
            assert!(!game.cells().contains(&3));
            assert!([2, 4].contains(&(mass(&game) - before)));
        }
    }

    #[test]
    fn all_directions_merge_toward_the_requested_edge() {
        for direction in [GameDirection::Left, GameDirection::Right, GameDirection::Up, GameDirection::Down] {
            let mut cells = [0; CELLS];
            for line in 0..SIDE {
                cells[index(direction, line, 1)] = line as u8 + 1;
                cells[index(direction, line, 2)] = line as u8 + 1;
            }
            let mut game = board(cells);
            assert!(game.slide(direction));
            for line in 0..SIDE {
                assert_eq!(game.cells()[index(direction, line, 0)], line as u8 + 2);
            }
            assert_eq!(game.cells().iter().filter(|&&cell| cell > 0).count(), SIDE + 1);
            assert_eq!(game.score(), 4 + 8 + 16 + 32);
        }
    }

    #[test]
    fn blocked_moves_do_not_spawn_or_consume_randomness() {
        let mut game = board([1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let original = game.clone();
        assert!(!game.slide(GameDirection::Left));
        assert!(!game.slide(GameDirection::Up));
        assert_eq!(game, original);
        assert!(!game.can_undo());
    }

    #[test]
    fn undo_restores_randomness_but_retains_the_best_score() {
        let mut game = board([1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let original = game.saved();
        assert!(game.slide(GameDirection::Left));
        let moved = game.saved();
        assert!(game.undo());
        assert_eq!(game.cells(), &original.cells);
        assert_eq!(game.score(), original.score);
        assert_eq!(game.best(), 4);
        assert!(!game.undo());
        assert!(game.slide(GameDirection::Left));
        assert_eq!(game.saved(), moved);
    }

    #[test]
    fn reaching_2048_pauses_until_continue_and_can_be_undone() {
        let mut game = board([10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(game.slide(GameDirection::Left));
        assert_eq!(game.status(), GameStatus::Won);
        assert!(!game.slide(GameDirection::Down));
        assert!(game.keep_playing());
        assert_eq!(game.status(), GameStatus::Playing);
        assert!(game.undo());
        assert_eq!(game.cells()[0..2], [10, 10]);
        assert_eq!(game.best(), 2048);
    }

    #[test]
    fn a_full_board_can_still_merge_and_detects_game_over() {
        let mut game = board([1, 2, 1, 2, 2, 1, 2, 1, 1, 2, 1, 2, 2, 1, 2, 1]);
        assert_eq!(game.status(), GameStatus::Over);
        assert!(!game.slide(GameDirection::Left));
        game.position.cells[1] = 1;
        assert_eq!(game.status(), GameStatus::Playing);
        assert!(game.slide(GameDirection::Left));
    }

    #[test]
    fn restart_keeps_the_record_and_starts_with_two_tiles() {
        let mut game = board([2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        game.slide(GameDirection::Left);
        game.restart();
        assert_eq!(game.score(), 0);
        assert_eq!(game.best(), 8);
        assert_eq!(game.cells().iter().filter(|&&cell| cell != 0).count(), INITIAL_TILES);
        assert!(!game.can_undo());
    }

    #[test]
    fn saved_boards_resume_and_reject_invalid_data() {
        let original = Game2048::new(0);
        assert_eq!(Game2048::restore(original.saved()).unwrap().saved(), original.saved());
        let mut saved = original.saved();
        saved.cells[0] = MAX_EXPONENT + 1;
        assert!(Game2048::restore(saved).is_none());
        let mut saved = original.saved();
        saved.version += 1;
        assert!(Game2048::restore(saved).is_none());
        let mut saved = original.saved();
        saved.cells.fill(0);
        assert!(Game2048::restore(saved).is_none());
    }
}
