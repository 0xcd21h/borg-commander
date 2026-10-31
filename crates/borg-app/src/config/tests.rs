use super::*;

#[test]
fn missing_keys_fall_back_to_defaults() {
    let config: Config = toml::from_str("theme = \"aurora\"\nmenu-bar = true\n").unwrap_or_default();
    let prefs = config.preferences();
    assert!(prefs.menu_bar);
    assert_eq!(prefs.status_line, Preferences::default().status_line);
    assert_eq!(config.theme.as_deref(), Some("aurora"));
}

#[test]
fn captured_preferences_round_trip() {
    let prefs = Preferences { menu_bar: true, after_command: AfterCommand::ReturnToPanels, ..Preferences::default() };
    let original = Config { screensaver_after: Some(42), ..Config::default() };
    let unlocked = ["cube".to_owned()];
    let text = toml::to_string(&original.capture(Some("ember"), &prefs, &unlocked)).unwrap_or_default();
    let back: Config = toml::from_str(&text).unwrap_or_default();
    assert_eq!(back.preferences(), prefs);
    assert_eq!(back.unlocked(), unlocked);
    assert_eq!(back.screensaver_after(), Some(Duration::from_secs(42)));
    assert!(text.contains("return-to-panels = true"));
}

#[test]
fn search_settings_default_to_skipping_artifacts_and_survive_saving_preferences() {
    let config: Config = toml::from_str("[search]\nexclude = [\"*.min.js\"]\nignore-files = false\n").unwrap();
    let search = config.search();
    assert_eq!(search.exclude, ["*.min.js"]);
    assert!(search.artifacts && !search.ignore_files && search.history);
    assert!(Config::default().search().ignore_files);
    let saved = config.capture(None, &Preferences::default(), &[]);
    assert_eq!(saved.search(), search);
    let edited = borg_core::SearchSettings { exclude: vec!["Library/Caches/".into()], artifacts: false, ..search };
    let text = toml::to_string(&saved.with_search(&edited)).unwrap();
    assert_eq!(toml::from_str::<Config>(&text).unwrap().search(), edited);
    let untouched = Config::default().with_search(&borg_core::SearchSettings::default());
    assert!(!toml::to_string(&untouched).unwrap().contains("[search]"));
}

#[test]
fn retired_pixel_chrome_setting_loads_but_is_not_saved() {
    for enabled in [true, false] {
        let config: Config = toml::from_str(&format!("pixel-chrome = {enabled}\ntheme = \"neon\"\n")).unwrap();
        assert_eq!(config.preferences(), Preferences::default());
        assert_eq!(config.theme.as_deref(), Some("neon"));
        let captured = config.capture(config.theme.as_deref(), &config.preferences(), &[]);
        assert!(!toml::to_string(&captured).unwrap().contains("pixel-chrome"));
    }
}

#[test]
fn rejects_unknown_keys() {
    assert!(toml::from_str::<Config>("colour = 1").is_err());
}

#[test]
fn old_switches_are_ignored_and_new_defaults_are_saved() {
    let config: Config =
        toml::from_str("animations = true\nstartup-animation = true\ndelight = false\nicons = false\n").unwrap();
    let prefs = config.preferences();
    assert_eq!(prefs, Preferences::default());
    assert_eq!(prefs.after_command, AfterCommand::ReturnToPanels);
    assert!(prefs.screensaver);
    assert!(!prefs.nerd_font);
    let saved = toml::to_string(&config.capture(None, &prefs, &[])).unwrap();
    for old in ["animations", "startup-animation", "delight", "icons"] {
        assert!(!saved.contains(old));
    }
    assert!(saved.contains("icon-style = \"auto\""));
}

#[test]
fn automatic_icons_follow_the_terminal() {
    let config = Config::default();
    assert!(config.preferences_for_terminal(Some("ghostty"), Some("xterm-256color")).nerd_font);
    assert!(config.preferences_for_terminal(None, Some("xterm-ghostty")).nerd_font);
    assert!(!config.preferences_for_terminal(Some("Apple_Terminal"), Some("xterm-256color")).nerd_font);
    assert!(!config.preferences_for_terminal(None, Some("xterm-256color")).nerd_font);
}

#[test]
fn saved_fallback_migrates_to_auto_without_locking_icons_to_one_terminal() {
    let config: Config = toml::from_str("nerd-font = false\n").unwrap();
    let ghostty = config.preferences_for_terminal(Some("ghostty"), None);
    assert!(ghostty.nerd_font);
    let saved = toml::to_string(&config.capture(None, &ghostty, &[])).unwrap();
    assert!(!saved.contains("nerd-font"));
    let reloaded: Config = toml::from_str(&saved).unwrap();
    assert_eq!(reloaded.icon_style, Some(IconStyle::Auto));
    assert!(reloaded.preferences_for_terminal(Some("ghostty"), None).nerd_font);
    assert!(!reloaded.preferences_for_terminal(Some("Apple_Terminal"), None).nerd_font);
}

#[test]
fn explicit_icon_style_overrides_terminal_detection() {
    let portable: Config = toml::from_str("icon-style = \"portable\"\n").unwrap();
    assert!(!portable.preferences_for_terminal(Some("ghostty"), None).nerd_font);
    let nerd: Config = toml::from_str("icon-style = \"nerd\"\n").unwrap();
    assert!(nerd.preferences_for_terminal(Some("Apple_Terminal"), None).nerd_font);
    let legacy: Config = toml::from_str("nerd-font = true\n").unwrap();
    let prefs = legacy.preferences_for_terminal(Some("Apple_Terminal"), None);
    assert!(prefs.nerd_font);
    assert_eq!(legacy.capture(None, &prefs, &[]).icon_style, Some(IconStyle::Nerd));
}

#[test]
fn git_is_enabled_by_default_and_the_toggle_survives_saving_other_preferences() {
    assert!(Config::default().preferences().git_plugin);
    let config: Config = toml::from_str("[plugins]\ngit = false\n").unwrap();
    let mut prefs = config.preferences();
    assert!(!prefs.git_plugin);
    prefs.status_line = false;
    let saved = toml::to_string(&config.capture(None, &prefs, &[])).unwrap();
    let restored: Config = toml::from_str(&saved).unwrap();
    assert!(!restored.preferences().git_plugin);
    assert!(!restored.preferences().status_line);
    assert!(toml::from_str::<Config>("[plugins]\ngti = false\n").is_err());
}

#[test]
fn saved_language_survives_other_preference_changes() {
    for language in borg_i18n::Locale::ALL {
        let source = format!("language = {:?}\n", language.code());
        let config: Config = toml::from_str(&source).unwrap();
        let mut prefs = config.preferences();
        assert_eq!(prefs.language, language);
        prefs.menu_bar = true;
        let saved = config.capture(None, &prefs, &[]);
        let restored: Config = toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
        assert_eq!(restored.preferences().language, language);
        assert!(restored.preferences().menu_bar);
    }
}
