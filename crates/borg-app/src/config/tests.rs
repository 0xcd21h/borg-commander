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
fn rejects_unknown_keys() {
    assert!(toml::from_str::<Config>("colour = 1").is_err());
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
fn explicit_icon_style_overrides_terminal_detection() {
    let portable: Config = toml::from_str("icon-style = \"portable\"\n").unwrap();
    assert!(!portable.preferences_for_terminal(Some("ghostty"), None).nerd_font);
    let nerd: Config = toml::from_str("icon-style = \"nerd\"\n").unwrap();
    assert!(nerd.preferences_for_terminal(Some("Apple_Terminal"), None).nerd_font);
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

#[test]
fn captures_keep_free_only_the_space_the_network_section_asks_for() {
    assert_eq!(Config::default().capture_keep_free_mib(), None);
    let config: Config = toml::from_str("[network]\nkeep-free-mib = 512\n").unwrap();
    assert_eq!(config.capture_keep_free_mib(), Some(512));
    let saved = toml::to_string(&config.capture(None, &Preferences::default(), &[])).unwrap();
    assert!(saved.contains("keep-free-mib = 512"), "{saved}");
    assert!(toml::from_str::<Config>("[network]\nkeep-free-mib = \"512M\"\n").is_err(), "a number of MiB");
}

#[test]
fn the_hotlist_keeps_home_as_a_tilde_and_survives_saving_preferences() {
    let home = PathBuf::from(env::var_os(ENV_HOME).expect("a home folder"));
    let mut config: Config = toml::from_str("[folders]\nhotlist = [\"/srv/www\"]\n").unwrap();
    assert!(config.folder_history());
    config.set_hotlist(&[PathBuf::from("/srv/www"), home.join("Projects")]);
    let saved = config.capture(None, &Preferences::default(), &[]);
    let text = toml::to_string(&saved).unwrap();
    assert!(text.contains("hotlist = [\"/srv/www\", \"~/Projects\"]"), "{text}");
    assert_eq!(toml::from_str::<Config>(&text).unwrap().hotlist(), [PathBuf::from("/srv/www"), home.join("Projects")]);
    let off: Config = toml::from_str("[folders]\nhistory = false\n").unwrap();
    assert!(!off.folder_history() && off.hotlist().is_empty());
}

/// A configuration Borg cannot read starts it on its defaults. Saving a preference then keeps
/// the file the user wrote under another name, where it used to be written over.
#[test]
fn saving_keeps_a_configuration_borg_could_not_read_beside_the_new_one() {
    let folder = tempfile::tempdir().unwrap();
    let dir = ConfigDir(folder.path().to_path_buf());
    let written_by_hand = "theme = \"neon\"\n[watch\nfreeze = false\n";
    std::fs::write(dir.file(), written_by_hand).unwrap();
    assert!(dir.load().is_err());

    dir.save(&Config::default()).unwrap();
    assert!(dir.load().is_ok());
    let aside = folder.path().join(CONFIG_ASIDE);
    assert_eq!(std::fs::read_to_string(&aside).unwrap(), written_by_hand);

    std::fs::write(dir.file(), written_by_hand).unwrap();
    dir.save(&Config::default()).unwrap();
    let kept = std::fs::read_dir(folder.path()).unwrap().count();
    assert_eq!(kept, 3, "a second unreadable file is kept under a name of its own");

    dir.save(&Config::default()).unwrap();
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), kept, "a readable one is replaced as before");
}

/// N in the update offer turns the daily check off: the file keeps what the user wrote, and the
/// next check reads it as off.
#[test]
fn turning_the_update_check_off_keeps_the_rest_of_the_file() {
    let folder = tempfile::tempdir().unwrap();
    let dir = ConfigDir(folder.path().to_path_buf());
    std::fs::write(dir.file(), "# mine\ntheme = \"neon\"\n").unwrap();
    let mut config = dir.load().unwrap();
    assert!(config.update_check(), "on unless the file says otherwise");
    config.update_check = Some(false);
    dir.save(&config).unwrap();
    let text = std::fs::read_to_string(dir.file()).unwrap();
    assert!(text.contains("# mine") && text.contains("update-check = false"), "{text}");
    assert!(!dir.load().unwrap().update_check());
}

fn remarked(text: &str) -> (Config, Vec<Remark>) {
    let loaded = document::load::<Config>(text).unwrap();
    (loaded.settings, loaded.remarks)
}

/// One setting Borg cannot use costs that setting alone: the rest of the file still applies,
/// and Borg says which line is wrong and how.
#[test]
fn a_setting_borg_cannot_use_costs_only_itself() {
    let text =
        "theme = \"neon\"\ncolour = 1\nscreensaver-after = \"soon\"\n\n[watch]\nlearning-days = 3\nfreez = true\n";
    let (config, remarks) = remarked(text);
    assert_eq!(config.theme.as_deref(), Some("neon"));
    assert_eq!(config.screensaver_after, None);
    assert_eq!(config.watch.as_ref().map(|watch| watch.learning_days), Some(3));
    assert!(
        matches!(&remarks[..], [
        Remark::Unknown { key: colour, line: 2 },
        Remark::Invalid { key: after, line: 3, .. },
        Remark::Unknown { key: freez, line: 7 },
    ] if colour == "colour" && after == "screensaver-after" && freez == "watch.freez"),
        "{remarks:?}"
    );
}

/// A section that needs two settings and has one is left out whole, and nothing else is.
#[test]
fn a_section_missing_a_part_is_left_out_whole() {
    let text = "[watch]\non = true\n\n[watch.notify.telegram]\ntoken = \"t\"\n";
    let (config, remarks) = remarked(text);
    let watch = config.watch.unwrap();
    assert!(watch.on);
    assert!(watch.notify.is_none_or(|notify| notify.telegram.is_none()));
    assert!(matches!(&remarks[..], [Remark::Invalid { key, .. }] if key == "watch.notify.telegram"), "{remarks:?}");
}

/// Settings Borg has retired load without complaint about a mistake: one that was dropped is
/// said to be unused, and one that was reshaped is read as what it became.
#[test]
fn retired_settings_are_dropped_or_read_as_what_they_became() {
    let (config, remarks) = remarked("animations = true\nnerd-font = true\npixel-chrome = false\n");
    assert_eq!(config.icon_style, Some(IconStyle::Nerd));
    assert_eq!(
        remarks,
        [
            Remark::Retired { key: "animations".into() },
            Remark::Retired { key: "pixel-chrome".into() },
            Remark::Moved { key: "nerd-font".into(), now: "icon-style".into() },
        ]
    );
    let (config, remarks) = remarked("nerd-font = false\n");
    assert_eq!((config.icon_style, remarks), (None, vec![Remark::Retired { key: "nerd-font".into() }]));
    let (config, _) = remarked("nerd-font = true\nicon-style = \"portable\"\n");
    assert_eq!(config.icon_style, Some(IconStyle::Portable), "what the user wrote for the new setting stands");
}

/// Saving a setting changes that setting in the file. The user's comments, the order of their
/// settings and a setting Borg does not know all stay, and a retired setting goes.
#[test]
fn saving_changes_what_changed_and_keeps_the_rest_of_the_file() {
    let text = "# my settings\ntheme = \"neon\"   # the bright one\ncolour = 1\nanimations = true\n\n[watch]\n# a week is too short here\nlearning-days = 14\n";
    let (mut config, _) = remarked(text);
    config.theme = Some("paper".into());
    config.menu_bar = Some(false);
    let saved = document::save(text, &config).unwrap();
    for kept in [
        "# my settings\n",
        "theme = \"paper\"   # the bright one\n",
        "colour = 1\n",
        "# a week is too short here\nlearning-days = 14\n",
        "menu-bar = false\n",
    ] {
        assert!(saved.contains(kept), "{kept:?} is missing from:\n{saved}");
    }
    assert!(!saved.contains("animations"), "{saved}");
    let (again, remarks) = remarked(&saved);
    assert_eq!(again, config);
    assert!(matches!(&remarks[..], [Remark::Unknown { key, .. }] if key == "colour"), "{remarks:?}");
    assert_eq!(document::save(&saved, &config).unwrap(), saved, "saving the same settings changes nothing");
}

#[test]
fn a_new_file_holds_what_was_saved_and_a_file_that_is_not_toml_fails() {
    let config = Config { theme: Some("neon".into()), watch: Some(Watch::default()), ..Config::default() };
    let saved = document::save("", &config).unwrap();
    assert_eq!(remarked(&saved), (config, Vec::new()));
    assert!(document::load::<Config>("theme = \n").is_err());
}

/// The sample in the guide is a configuration Borg takes whole, so it cannot describe a
/// setting that does not exist or a value that does not fit.
#[test]
fn the_guides_sample_configuration_loads_without_a_remark() {
    const GUIDE: &str = include_str!("../../../../docs/GUIDE.md");
    const OPENING: &str = "```toml\n";
    let sample = GUIDE
        .split(OPENING)
        .skip(1)
        .filter_map(|rest| rest.split("```").next())
        .find(|block| block.contains("\ntheme = ") || block.starts_with("theme = "))
        .expect("the guide has a sample configuration");
    let (_, remarks) = remarked(sample);
    assert_eq!(remarks, Vec::new());
}
