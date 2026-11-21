use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::memory_strings::{credential, web_token};

const HIDDEN: &str = "••••";
const QUOTES: [char; 2] = ['"', '\''];

/// Parts of a name that mark the value given to it as a secret.
const SECRET_NAMES: &str = r"pass|secret|token|api[_-]?key|access[_-]?key|private[_-]?key|credential|authorization";
/// Words that come before the secret itself in an `Authorization` header.
const AUTH_SCHEMES: &str = r"bearer|basic|token|digest|negotiate";
/// A value: quoted, or up to the next space, quote or separator.
const VALUE: &str = r#""[^"]*"|'[^']*'|[^\s"',;}]+"#;
/// The rest of a command, up to where the next one would start.
const SAME_COMMAND: &str = r"[^\n|;&]*?";
/// Commands whose `-p` takes the password glued to it, as in `-pS3cret`.
const GLUED_PASSWORD_COMMANDS: &str =
    r"mysql|mysqldump|mysqladmin|mysqlimport|mysqlcheck|mysqlshow|mariadb|mariadb-dump";
/// Commands whose `-p` takes the password, glued to it or as the next word.
const PASSWORD_OPTION_COMMANDS: &str = r"sshpass|(?:docker|podman|nerdctl)[ \t]+login";

const PRIVATE_KEY: &str =
    r"(?s)(-----BEGIN [A-Z0-9 ]*PRIVATE KEY[A-Z ]*-----)(.*?)(-----END [A-Z0-9 ]*PRIVATE KEY[A-Z ]*-----|\z)";
const URL_PASSWORD: &str = r"(?i)(\b[a-z][a-z0-9+.-]*://[^\s/:@]*:)([^\s/@]+)(@)";

/// Where secrets sit in text. Each pattern captures what comes before a secret, the secret,
/// and what may follow it.
static PLACES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        PRIVATE_KEY.to_owned(),
        URL_PASSWORD.to_owned(),
        format!(r#"(\bcurl\b{SAME_COMMAND}[ \t](?:-u[ \t]*|--user[ \t=]+)["']?[^\s:"']*:)([^\s"']+)"#),
        format!(r"(\b(?:{GLUED_PASSWORD_COMMANDS})\b{SAME_COMMAND}[ \t]-p)(\S+)"),
        format!(r"(\b(?:{PASSWORD_OPTION_COMMANDS})\b{SAME_COMMAND}[ \t]-p[ \t]*)(\S+)"),
        format!(
            r#"(?i)(\b[\w.-]*(?:{SECRET_NAMES})[\w.-]*["']?[ \t]*[=:][ \t]*(?:(?:{AUTH_SCHEMES})[ \t]+)?)({VALUE})"#
        ),
        format!(r"(?i)([ \t]--?[\w.-]*(?:{SECRET_NAMES})[\w.-]*[ \t]+)({VALUE})"),
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("a pattern for secrets is valid"))
    .collect()
});

/// Runs of the characters tokens are made of.
static WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_.-]+").expect("the pattern for words is valid"));

/// The text with the passwords, tokens and keys it recognizes replaced by a mark: values of
/// options and settings named like secrets, passwords in URLs and in common command lines,
/// well-known tokens, and private keys.
pub fn hidden(text: &str) -> String {
    let placed = PLACES.iter().fold(text.to_owned(), |text, place| place.replace_all(&text, hide).into_owned());
    WORDS.replace_all(&placed, |word: &Captures| hidden_word(&word[0])).into_owned()
}

fn hide(found: &Captures) -> String {
    let part = |group| found.get(group).map_or("", |part| part.as_str());
    format!("{}{}{}", part(1), masked(part(2)), part(3))
}

/// The value replaced by the mark, keeping the quotes and the space around it.
fn masked(value: &str) -> String {
    let edge = |c: char| c.is_whitespace() || QUOTES.contains(&c);
    let inner = value.trim_matches(edge);
    if inner.is_empty() {
        return value.to_owned();
    }
    let start = value.len() - value.trim_start_matches(edge).len();
    format!("{}{HIDDEN}{}", &value[..start], &value[start + inner.len()..])
}

fn hidden_word(word: &str) -> String {
    if web_token(word) {
        return HIDDEN.to_owned();
    }
    word.split('.').map(|part| if credential(part) { HIDDEN } else { part }).collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB_TOKEN_START: &str = "ghp_";
    const GITHUB_TOKEN_REST: &str = "0123456789abcdefghijABCDEFGHIJ0123456789";
    const WEB_TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJtZSJ9.c2lnbmF0dXJlLXNpZ25hdHVyZQ";

    #[test]
    fn values_named_like_secrets_are_hidden() {
        for (written, logged) in [
            ("$ PGPASSWORD=S3cret psql -h db", "$ PGPASSWORD=•••• psql -h db"),
            ("$ docker run -e POSTGRES_PASSWORD='two words' db", "$ docker run -e POSTGRES_PASSWORD='••••' db"),
            ("$ deploy --api-key=abc123 --verbose", "$ deploy --api-key=•••• --verbose"),
            ("$ deploy --password \"two words\" --verbose", "$ deploy --password \"••••\" --verbose"),
            ("$ vault login -method=userpass password=S3cret", "$ vault login -method=userpass password=••••"),
            (
                "$ curl -d '{\"user\":\"me\",\"password\":\"S3cret\"}' https://shop.test/login",
                "$ curl -d '{\"user\":\"me\",\"password\":\"••••\"}' https://shop.test/login",
            ),
            (
                "$ curl -H \"Authorization: Bearer abc.def\" https://shop.test",
                "$ curl -H \"Authorization: Bearer ••••\" https://shop.test",
            ),
            ("$ curl -H 'X-Api-Key: abc123' https://shop.test", "$ curl -H 'X-Api-Key: ••••' https://shop.test"),
            ("$ openssl enc -d -in a.enc -pass pass:S3cret", "$ openssl enc -d -in a.enc -pass ••••"),
        ] {
            assert_eq!(hidden(written), logged);
        }
    }

    #[test]
    fn passwords_in_urls_and_common_command_lines_are_hidden() {
        for (written, logged) in [
            ("$ git clone https://me:S3cret@git.test/shop.git", "$ git clone https://me:••••@git.test/shop.git"),
            ("$ curl -u admin:S3cret https://shop.test", "$ curl -u admin:•••• https://shop.test"),
            ("$ curl --user=admin:S3cret https://shop.test", "$ curl --user=admin:•••• https://shop.test"),
            ("$ mysql -u root -pS3cret shop", "$ mysql -u root -p•••• shop"),
            ("$ sshpass -p 'S3cret' ssh web", "$ sshpass -p '••••' ssh web"),
            ("$ docker login -u me -p S3cret registry.test", "$ docker login -u me -p •••• registry.test"),
        ] {
            assert_eq!(hidden(written), logged);
        }
    }

    #[test]
    fn well_known_tokens_and_private_keys_are_hidden() {
        let token = [GITHUB_TOKEN_START, GITHUB_TOKEN_REST].concat();
        let pushed = format!("$ git push https://{token}@github.test/me/shop.git");
        assert_eq!(hidden(&pushed), "$ git push https://••••@github.test/me/shop.git");
        assert_eq!(hidden(&format!("$ curl https://shop.test/?jwt={WEB_TOKEN}")), "$ curl https://shop.test/?jwt=••••");
        let begin = ["-----BEGIN OPENSSH ", "PRIVATE KEY-----"].concat();
        let end = "-----END OPENSSH PRIVATE KEY-----";
        let typed = format!("typed into the console\ncat > id <<EOF\n{begin}\nb3BlbnNz\nAAAA\n{end}\nEOF");
        assert_eq!(hidden(&typed), format!("typed into the console\ncat > id <<EOF\n{begin}\n••••\n{end}\nEOF"));
        let cut = ["-----BEGIN ", "PRIVATE KEY-----"].concat();
        assert_eq!(hidden(&format!("$ cat <<EOF\n{cut}\nMIIE")), format!("$ cat <<EOF\n{cut}\n••••"));
    }

    #[test]
    fn entries_without_secrets_stay_as_they_are() {
        for entry in [
            "ran in ~/srv/shop\n$ make test\n[exit 0]",
            "ran in ~\n$ ssh -p 2222 web uptime\n[exit 0]",
            "$ mkdir -p build && mysql -u root -p shop",
            "$ git commit --author=\"Ann <ann@shop.test>\" -m fix",
            "$ kubectl get secret web -o yaml",
            "read /srv/shop/secrets/README",
            "declined in ~/srv\n$ rm -rf build",
            "$ export PASSWORD=",
        ] {
            assert_eq!(hidden(entry), entry);
        }
    }
}
