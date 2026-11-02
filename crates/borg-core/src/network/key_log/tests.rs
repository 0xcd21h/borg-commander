use super::*;

const KEY_LOG: &str = "/work/it's here/capture-20260101-120000.keylog";

fn typed(line: CommandLine) -> String {
    text(&line)
}

#[test]
fn each_shell_sets_and_clears_the_variable_its_own_way() {
    let posix = ShellKeyLog::new(Dialect::Posix, None);
    assert_eq!(
        typed(posix.set_line(Path::new(KEY_LOG))),
        r"export SSLKEYLOGFILE='/work/it'\''s here/capture-20260101-120000.keylog'"
    );
    assert_eq!(typed(posix.restore_line()), "unset SSLKEYLOGFILE");
    let fish = ShellKeyLog::new(Dialect::Fish, None);
    assert_eq!(
        typed(fish.set_line(Path::new(KEY_LOG))),
        r"set -gx SSLKEYLOGFILE '/work/it'\''s here/capture-20260101-120000.keylog'"
    );
    assert_eq!(typed(fish.restore_line()), "set -e SSLKEYLOGFILE");
}

#[test]
fn the_shell_gets_back_the_value_borg_started_with() {
    let inherited = Some(PathBuf::from("/home/me/tls.keylog"));
    assert_eq!(
        typed(ShellKeyLog::new(Dialect::Posix, inherited.clone()).restore_line()),
        "export SSLKEYLOGFILE='/home/me/tls.keylog'"
    );
    assert_eq!(
        typed(ShellKeyLog::new(Dialect::Fish, inherited).restore_line()),
        "set -gx SSLKEYLOGFILE '/home/me/tls.keylog'"
    );
}

#[test]
fn other_terminals_put_the_variable_in_front_of_the_program() {
    assert_eq!(
        key_log_prefix(Path::new(KEY_LOG)),
        r"SSLKEYLOGFILE='/work/it'\''s here/capture-20260101-120000.keylog' "
    );
}

#[test]
fn bash_zsh_and_sh_take_export_and_fish_takes_set() {
    assert_eq!(Dialect::of(ShellKind::Fish), Dialect::Fish);
    for kind in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Other] {
        assert_eq!(Dialect::of(kind), Dialect::Posix);
    }
}
