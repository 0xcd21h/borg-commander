use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use borg_gfx::Picture;
use borg_testkit::{Fixture, Recorder, Session, keys};
use crossterm::event::KeyCode;

const USAGE: &str = "usage: demo <path-to-borg-binary> <output.gif>";
const FRAME: Duration = Duration::from_millis(90);
const TAIL: Duration = Duration::from_secs(3);
/// The startup intro, from the logo tearing in until the panels appear.
const INTRO: Duration = Duration::from_millis(1500);
const COLS: u16 = 132;
const ROWS: u16 = 36;
const CONFIG: &str = "theme = \"neon\"\nnerd-font = true\nreturn-to-panels = false\nmenu-bar = true\n";
const TYPE_DELAY: Duration = Duration::from_millis(45);
const SHORT: Duration = Duration::from_millis(350);
const BEAT: Duration = Duration::from_millis(700);
const LONG: Duration = Duration::from_millis(1400);
const PICTURE_SIZE: (u32, u32) = (480, 270);
const PROJECT: [&str; 2] = ["projects", "borg"];
const ASSETS: &str = "assets";
const SCRIPT: &str = "build.sh";
const SCRIPT_BODY: &str = "#!/bin/sh\ncargo build --release\n";
const EXECUTABLE: u32 = 0o755;
const PICTURES: [(&str, f32); 3] = [("sunset.png", 0.0), ("tide.png", 1.7), ("nebula.png", 3.1)];

const SOURCES: [(&str, &str); 4] = [
    ("src/main.rs", "fn main() {\n    borg::run();\n}\n"),
    ("src/app.rs", "pub struct App;\n"),
    ("src/ui.rs", "pub fn draw() {}\n"),
    ("src/theme.rs", "pub struct Theme;\n"),
];
const FILES: [(&str, &str); 13] = [
    ("Cargo.toml", "[package]\nname = \"borg\"\n"),
    ("Cargo.lock", "version = 4\n"),
    (
        "README.md",
        "# Borg\n\nA file manager with a shell behind the panels.\n\n## Quick preview\n\n- **Ctrl-Q** opens the preview\n- **Tab** focuses the document\n- Press `s` to switch to source\n\n```rust\nfn main() {\n    println!(\"Hello, Borg!\");\n}\n```\n",
    ),
    ("LICENSE", "MIT\n"),
    ("Dockerfile", "FROM rust:1\n"),
    ("package.json", "{ \"name\": \"borg-web\" }\n"),
    ("tsconfig.json", "{}\n"),
    ("next.config.js", "module.exports = {};\n"),
    ("notes.txt", "We are the Borg.\nLower your shields and surrender your files.\n"),
    ("docs/guide.md", "# Guide\n"),
    ("docs/architecture.md", "# Architecture\n"),
    ("CHANGELOG.md", "# 0.3.0\n\nMouse navigation, console scrollback, and fast terminal chrome.\n"),
    ("Makefile", "build:\n\tcargo build --release\n"),
];

fn picture(seed: f32) -> Result<Vec<u8>> {
    let (width, height) = PICTURE_SIZE;
    let pixel = |x: u32, y: u32| {
        let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
        let (dx, dy) = (u - 0.5, (v - 0.62) * 0.56);
        let sun = (dx * dx + dy * dy).sqrt() < 0.12;
        let wave = ((u * 18.0 + seed).sin() * 0.03 + 0.78 - v).signum() < 0.0;
        let sky = [255.0 * (0.2 + 0.8 * v), 60.0 + 90.0 * v * seed.cos().abs(), 180.0 * (1.0 - v) + 50.0];
        let [r, g, b] = if wave {
            [20.0, 30.0 + 40.0 * v, 80.0 + 60.0 * v]
        } else if sun {
            [255.0, 214.0, 96.0]
        } else {
            sky
        };
        [r as u8, g as u8, b as u8, u8::MAX]
    };
    let rgba = (0..height).flat_map(|y| (0..width).flat_map(move |x| pixel(x, y))).collect();
    Ok(Picture::from_rgba(width, height, rgba)?.png()?.to_vec())
}

fn write(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(fs::write(path, body)?)
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=Borg Demo", "-c", "user.email=demo@example.test"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()?;
    anyhow::ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

fn project(fixture: &Fixture) -> Result<PathBuf> {
    let root = PROJECT.iter().fold(fixture.home(), |path, part| path.join(part));
    SOURCES.iter().chain(&FILES).try_for_each(|(path, body)| write(&root.join(path), body.as_bytes()))?;
    let script = root.join(SCRIPT);
    write(&script, SCRIPT_BODY.as_bytes())?;
    fs::set_permissions(&script, fs::Permissions::from_mode(EXECUTABLE))?;
    for (name, seed) in PICTURES {
        write(&root.join(ASSETS).join(name), &picture(seed)?)?;
    }
    let releases = fixture.home().join("releases");
    for name in [
        "borg-linux-arm64.tar.gz",
        "borg-linux-x64.tar.gz",
        "borg-macos-arm64.tar.gz",
        "borg-macos-x64.tar.gz",
        "checksums.txt",
        "release-notes.md",
    ] {
        write(&releases.join(name), b"A fictional release artifact for the demo.\n")?;
    }
    let panel =
        |dir: &Path| format!("dir = {:?}\nmode = \"full\"\nsort = \"name\"\nreverse = false\n", dir.to_string_lossy());
    let session = format!("active = \"left\"\nsplit = 0.5\n[left]\n{}[right]\n{}", panel(&root), panel(&releases));
    write(&fixture.state().join("session.toml"), session.as_bytes())?;
    fixture.configure(CONFIG)?;
    git(&root, &["init", "-b", "main"])?;
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-qm", "Start the demo project"])?;
    write(&root.join("src/main.rs"), b"fn main() {\n    println!(\"Welcome to Borg\");\n    borg::run();\n}\n")?;
    git(&root, &["add", "src/main.rs"])?;
    let readme = FILES.iter().find(|(name, _)| *name == "README.md").unwrap().1;
    write(&root.join("README.md"), format!("{readme}\nGit status appears beside your files.\n").as_bytes())?;
    write(&root.join("roadmap.md"), b"# Next steps\n\n- Review changes\n- Prepare the release\n")?;
    Ok(root)
}

struct Script {
    app: Session,
    recorder: Recorder,
}

impl Script {
    fn press(&mut self, key: impl AsRef<[u8]>, hold: Duration) -> Result<&mut Self> {
        self.app.send(key)?;
        self.recorder.record(&mut self.app, hold)?;
        Ok(self)
    }

    fn type_text(&mut self, text: &str, hold: Duration) -> Result<&mut Self> {
        for c in text.chars() {
            self.press(c.to_string(), TYPE_DELAY)?;
        }
        self.recorder.record(&mut self.app, hold)?;
        Ok(self)
    }

    fn click(&mut self, label: &str, hold: Duration) -> Result<&mut Self> {
        let screen = self.app.wait_text(label)?;
        let (x, y) = screen.find(label).with_context(|| format!("finding {label} for a mouse click"))?;
        self.press(format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", x + 1, y + 1, x + 1, y + 1), hold)
    }

    fn find(&mut self, query: &str) -> Result<&mut Self> {
        self.press(keys::ctrl('s'), SHORT)?.type_text(query, SHORT)?.press(keys::enter(), SHORT)
    }
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let binary = fs::canonicalize(PathBuf::from(args.next().context(USAGE)?))?;
    let output = PathBuf::from(args.next().context(USAGE)?);

    let fixture = Fixture::new_in(Path::new("/tmp"))?;
    let root = project(&fixture)?;
    let app = Session::builder(&binary, &fixture).cwd(root).size(COLS, ROWS).label("demo").spawn()?;
    let mut script = Script { app, recorder: Recorder::new(FRAME) };

    script.app.wait_until("the glitch intro", |screen| screen.contains("█"))?;
    script.recorder.record(&mut script.app, INTRO)?;
    script.app.wait_text("README.md")?;
    script.app.wait_text("checksums.txt")?;
    script.app.wait_text("Git main")?;
    script.press(keys::down(), BEAT)?.press(keys::down(), SHORT)?;
    script
        .press(keys::ctrl('s'), SHORT)?
        .type_text("js", BEAT)?
        .press(keys::down(), BEAT)?
        .press(keys::enter(), SHORT)?;

    script.find("src")?.press(keys::enter(), LONG)?;
    script.press(keys::down(), SHORT)?.press(keys::ctrl('q'), LONG)?.press(keys::down(), LONG)?;
    script.press(keys::ctrl('q'), SHORT)?.press(keys::key(KeyCode::Home), SHORT)?.press(keys::enter(), BEAT)?;

    script.find("README")?;
    script.press(keys::key(KeyCode::Insert), SHORT)?.press(keys::key(KeyCode::Insert), BEAT)?;
    script.click("Git", BEAT)?.click("Diff selection", LONG)?.press(keys::esc(), BEAT)?;
    script.click("File", BEAT)?.press(keys::down(), SHORT)?.press(keys::esc(), SHORT)?;
    script.click("Options", BEAT)?.click("Theme…", BEAT)?;
    for key in [keys::up(), keys::down(), keys::down()] {
        script.press(key, LONG)?;
    }
    script.press(keys::esc(), BEAT)?;
    script.click("Options", SHORT)?.click("Settings…", LONG)?.press(keys::esc(), BEAT)?;
    script.find("README")?.press(keys::ctrl('q'), LONG)?.press(keys::tab(), BEAT)?;
    script.press("s", LONG)?.press("s", BEAT)?.press(keys::tab(), SHORT)?.press(keys::ctrl('q'), SHORT)?;

    script.type_text("printf 'Scan complete: deck %02d\\n' {1..70}", SHORT)?.press(keys::enter(), LONG)?;
    script.app.wait_text("Borg console")?;
    script.press("\x1b[<64;12;12M", LONG)?;
    script
        .press(keys::with(KeyCode::End, crossterm::event::KeyModifiers::SHIFT), SHORT)?
        .press(keys::ctrl('o'), LONG)?;

    script.find("assets")?.press(keys::enter(), BEAT)?.find("sunset")?;
    script.press(keys::f(3), LONG)?.press(keys::right(), LONG)?.press(keys::right(), LONG)?.press(keys::esc(), BEAT)?;
    script.press(keys::f(1), LONG)?.type_text("borg", LONG)?.press(keys::esc(), BEAT)?;
    script.press(keys::ctrl('q'), LONG)?;

    let screen = script.app.snapshot();
    for snapshot in script.recorder.snapshots() {
        let text = snapshot.lines().join("\n");
        anyhow::ensure!(
            !text.contains("/Users/") && !text.contains("/var/folders/"),
            "demo must not show host folders"
        );
    }
    screen.save_png(&output.with_extension("png"))?;

    let frames = script.recorder.frames();
    script.recorder.save_gif(&output, TAIL)?;
    println!("wrote {frames} frames to {}", output.display());
    Ok(())
}
