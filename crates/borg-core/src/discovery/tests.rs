use super::*;
use crate::{AppState, Effect, LineInput, Overlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
#[test]
fn tree_filters_keep_the_hierarchy_instead_of_becoming_flat_totals() {
    let now = std::time::Instant::now();
    let (mut state, _) = AppState::new(
        crate::Startup::at("/work".into()),
        100,
        30,
        crate::Settings::default(),
        crate::Appearance::new(vec![crate::ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    let mut form = SearchForm::new(&state, true);
    form.kind = SearchKind::FolderTree;
    form.files = crate::LineInput::new("20");
    let effects = search_key(&mut state, form, &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&effects[..], [crate::Effect::Search { request, .. }] if request.options.kind == SearchKind::FolderTree && request.options.minimum_files == 20)
    );
}

#[test]
fn searches_are_remembered_recalled_with_ctrl_p_and_bad_queries_keep_the_window_open() {
    let now = std::time::Instant::now();
    let (mut state, _) = AppState::new(
        crate::Startup::at("/work".into()),
        100,
        30,
        crate::Settings::default(),
        crate::Appearance::new(vec![crate::ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let mut form = SearchForm::new(&state, false);
    form.pattern = LineInput::new("ext:rs -test");
    let effects = search_key(&mut state, form, &enter);
    assert!(matches!(&effects[..], [Effect::SaveSearchHistory, Effect::Search { request, .. }]
        if request.skip.is_some() && request.query == "ext:rs -test"));
    let mut form = SearchForm::new(&state, false);
    form.pattern = LineInput::new("size:huge");
    search_key(&mut state, form, &enter);
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("the window stays open") };
    assert!(form.error.as_deref().is_some_and(|error| error.contains("size:huge")));
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    search_key(&mut state, form, &ctrl('p'));
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!(form.pattern.text(), "ext:rs -test");
    search_key(&mut state, form, &ctrl('n'));
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!(form.pattern.text(), "size:huge", "Ctrl-N returns to what was typed");
    let mut form = SearchForm::new(&state, false);
    form.pattern = LineInput::new("node_modules excluded:yes");
    let effects = search_key(&mut state, form, &enter);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Search { request, .. } if request.skip.is_none())));
}

#[test]
fn the_dialog_lists_the_latest_searches_follows_ctrl_p_and_takes_one_when_clicked() {
    let now = std::time::Instant::now();
    let (mut state, _) = AppState::new(
        crate::Startup::at("/work".into()),
        100,
        30,
        crate::Settings::default(),
        crate::Appearance::new(vec![crate::ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    for query in ["one", "two", "three", "four", "five", "six"] {
        state.search_history.record(query, std::path::Path::new("/work"));
    }
    state.search_history.record("seven", std::path::Path::new("/elsewhere"));
    let mut form = SearchForm::new(&state, false);
    form.pattern = LineInput::new("draft");
    let queries = |form: &SearchForm, state: &AppState| {
        form.recent(state).into_iter().map(|(at, past)| (at, past.query.clone())).collect::<Vec<_>>()
    };
    assert_eq!(
        queries(&form, &state),
        [(0, "six"), (1, "five"), (2, "four"), (3, "three"), (4, "two")].map(|(at, query)| (at, query.to_owned()))
    );
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    for _ in 0..7 {
        form.recall(&state, true);
    }
    assert_eq!(form.pattern.text(), "seven", "searches made elsewhere come after those made here");
    assert_eq!(queries(&form, &state).first().map(|(at, _)| *at), Some(2), "the list follows the recalled one");
    form.selected = MINIMUM_FIELD;
    state.overlay = Some(Overlay::Search(form));
    crate::update(&mut state, crate::Msg::Pointer(crate::Pointer::SearchRecent(1)), now);
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!((form.pattern.text(), form.selected), ("five", 0));
    search_key(&mut state, form, &ctrl('n'));
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!(form.pattern.text(), "six");
    search_key(&mut state, form, &ctrl('n'));
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!(form.pattern.text(), "draft", "what was typed comes back past the newest");
}

#[test]
fn a_contents_query_with_its_switches_searches_files_and_bad_ones_keep_the_window_open() {
    let now = std::time::Instant::now();
    let (mut state, _) = AppState::new(
        crate::Startup::at("/work".into()),
        100,
        30,
        crate::Settings::default(),
        crate::Appearance::new(vec![crate::ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    state.find_recent = vec!["fn main".into()];
    let press = |state: &mut AppState, key: KeyEvent| {
        let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
        search_key(state, form, &key)
    };
    let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let alt = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
    state.overlay = Some(Overlay::Search(SearchForm::new(&state, false)));
    press(&mut state, plain(KeyCode::Tab));
    press(&mut state, KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    press(&mut state, alt('r'));
    press(&mut state, alt('w'));
    press(&mut state, plain(KeyCode::Char('(')));
    let effects = press(&mut state, plain(KeyCode::Enter));
    assert!(effects.is_empty());
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("a bad regular expression keeps it open") };
    assert_eq!(form.selected, CONTENTS_FIELD);
    assert!(form.error.as_deref().is_some_and(|error| error.starts_with("Not a regular expression")));

    press(&mut state, plain(KeyCode::Backspace));
    let Some(Overlay::Search(mut form)) = state.overlay.clone() else { panic!("search window") };
    form.files = LineInput::new("3");
    state.overlay = Some(Overlay::Search(form));
    press(&mut state, plain(KeyCode::Enter));
    let Some(Overlay::Search(mut form)) = state.overlay.clone() else { panic!("a file count keeps it open") };
    assert_eq!(form.error.as_deref(), Some(tr!(contents_lists_files)));

    form.files = LineInput::new("");
    state.overlay = Some(Overlay::Search(form));
    press(&mut state, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT));
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!((form.kind, form.selected), (SearchKind::FolderTotals, KIND_FIELD), "size scans have no contents");
    press(&mut state, KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    press(&mut state, KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    let Some(Overlay::Search(form)) = state.overlay.clone() else { panic!("search window") };
    assert_eq!(form.selected, PATTERN_FIELD, "Shift-Tab passes over the contents");
    let effects = press(&mut state, plain(KeyCode::Enter));
    assert!(matches!(&effects[..], [.., Effect::Search { request, .. }]
        if request.options.kind == SearchKind::FolderTotals && request.options.contents.is_none()));

    let mut form = SearchForm::new(&state, false);
    form.contents.load(&borg_view::Query { text: "fn main".into(), case: false, words: true, regex: true });
    form.kind = SearchKind::Folders;
    form.minimum = LineInput::new("1k");
    state.overlay = Some(Overlay::Search(form));
    let effects = press(&mut state, plain(KeyCode::Enter));
    let expected = borg_view::Query { text: "fn main".into(), case: false, words: true, regex: true };
    assert!(matches!(&effects[..], [.., Effect::Search { request, .. }]
        if request.options.kind == SearchKind::Files
            && request.options.minimum == 1024
            && request.options.contents.as_deref() == Some(&expected)));
    assert_eq!(state.find_recent, ["fn main"]);
    let form = SearchForm::new(&state, false);
    assert_eq!(form.contents.query(), expected, "Ctrl-F on the results starts from their query");
}

#[test]
fn sizes_accept_units_but_reject_invalid_or_overflowing_input() {
    assert_eq!(parse_size("1.5 GiB"), Some(1_610_612_736));
    assert_eq!(parse_size("500m"), Some(524_288_000));
    for input in ["-1", "NaN", "1 elephant", "99999999999999999999T"] {
        assert_eq!(parse_size(input), None);
    }
}
