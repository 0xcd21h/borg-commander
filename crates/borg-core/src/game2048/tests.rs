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
    for (row, expected, score) in [([1, 1, 1, 1], [2, 2], 8), ([1, 1, 2, 0], [2, 2], 4), ([2, 1, 1, 0], [2, 2], 4)] {
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
