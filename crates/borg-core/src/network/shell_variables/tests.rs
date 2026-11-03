use super::*;
use std::{net::SocketAddr, path::PathBuf};

const KEY_LOG: &str = "/work/it's here/capture-20260101-120000.keylog";
const TRUST: &str = "/run/borg/trust-7-1.pem";

fn typed(line: Option<CommandLine>) -> String {
    text(&line.expect("a line"))
}

fn proxy() -> ProxyEndpoint {
    ProxyEndpoint { address: "127.0.0.1:8080".parse::<SocketAddr>().unwrap(), trust: PathBuf::from(TRUST) }
}

fn after(variables: ShellVariables, assignments: &Assignments) -> ShellVariables {
    ShellVariables { set: assignments.iter().map(|(name, _)| *name).collect(), ..variables }
}

#[test]
fn each_shell_sets_and_clears_the_key_log_its_own_way() {
    let keys = key_log_assignments(Path::new(KEY_LOG));
    let posix = ShellVariables::new(Dialect::Posix, BTreeMap::new());
    assert_eq!(
        typed(posix.set_line(&keys)),
        r"export SSLKEYLOGFILE='/work/it'\''s here/capture-20260101-120000.keylog'"
    );
    assert_eq!(typed(after(posix, &keys).restore_line()), "unset SSLKEYLOGFILE");
    let fish = ShellVariables::new(Dialect::Fish, BTreeMap::new());
    assert_eq!(
        typed(fish.set_line(&keys)),
        r"set -gx SSLKEYLOGFILE '/work/it'\''s here/capture-20260101-120000.keylog'"
    );
    assert_eq!(typed(after(fish, &keys).restore_line()), "set -e SSLKEYLOGFILE");
}

#[test]
fn the_shell_gets_back_the_value_borg_started_with() {
    let keys = key_log_assignments(Path::new(KEY_LOG));
    let inherited = BTreeMap::from([(KEY_LOG_VARIABLE, OsString::from("/home/me/tls.keylog"))]);
    assert_eq!(
        typed(after(ShellVariables::new(Dialect::Posix, inherited.clone()), &keys).restore_line()),
        "export SSLKEYLOGFILE='/home/me/tls.keylog'"
    );
    assert_eq!(
        typed(after(ShellVariables::new(Dialect::Fish, inherited), &keys).restore_line()),
        "set -gx SSLKEYLOGFILE '/home/me/tls.keylog'"
    );
}

#[test]
fn the_proxy_sets_every_variable_in_one_line_and_restores_each_as_it_was() {
    let assignments = proxy_assignments(&proxy());
    let inherited = BTreeMap::from([("SSL_CERT_FILE", OsString::from("/etc/ssl/cert.pem"))]);
    let posix = ShellVariables::new(Dialect::Posix, inherited.clone());
    assert_eq!(
        typed(posix.set_line(&assignments)),
        "export HTTPS_PROXY='http://127.0.0.1:8080' https_proxy='http://127.0.0.1:8080' \
         SSL_CERT_FILE='/run/borg/trust-7-1.pem' CURL_CA_BUNDLE='/run/borg/trust-7-1.pem' \
         REQUESTS_CA_BUNDLE='/run/borg/trust-7-1.pem' NODE_EXTRA_CA_CERTS='/run/borg/trust-7-1.pem'"
    );
    assert_eq!(
        typed(after(posix, &assignments).restore_line()),
        "export SSL_CERT_FILE='/etc/ssl/cert.pem'; unset HTTPS_PROXY https_proxy CURL_CA_BUNDLE REQUESTS_CA_BUNDLE NODE_EXTRA_CA_CERTS"
    );
    let fish = ShellVariables::new(Dialect::Fish, inherited);
    assert_eq!(
        typed(fish.set_line(&assignments[..2].to_vec())),
        "set -gx HTTPS_PROXY 'http://127.0.0.1:8080'; set -gx https_proxy 'http://127.0.0.1:8080'"
    );
    let fish = after(fish, &assignments[1..3].to_vec());
    assert_eq!(typed(fish.restore_line()), "set -gx SSL_CERT_FILE '/etc/ssl/cert.pem'; set -e https_proxy");
}

#[test]
fn other_terminals_put_the_variables_in_front_of_the_program() {
    assert_eq!(
        prefix(&key_log_assignments(Path::new(KEY_LOG))),
        r"SSLKEYLOGFILE='/work/it'\''s here/capture-20260101-120000.keylog' "
    );
    assert!(prefix(&proxy_assignments(&proxy())).starts_with("HTTPS_PROXY='http://127.0.0.1:8080' https_proxy="));
}

#[test]
fn bash_zsh_and_sh_take_export_and_fish_takes_set() {
    assert_eq!(Dialect::of(ShellKind::Fish), Dialect::Fish);
    for kind in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Other] {
        assert_eq!(Dialect::of(kind), Dialect::Posix);
    }
}
