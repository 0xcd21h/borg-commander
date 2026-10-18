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
const COLS: u16 = 100;
const ROWS: u16 = 26;
const CONFIG: &str = "startup-animation = true\ntheme = \"neon\"\nicons = true\nmenu-bar = false\n";
const TYPE_DELAY: Duration = Duration::from_millis(90);
const SHORT: Duration = Duration::from_millis(350);
const BEAT: Duration = Duration::from_millis(700);
const LONG: Duration = Duration::from_millis(1400);
const SPLASH: Duration = Duration::from_millis(2200);
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
const FILES: [(&str, &str); 11] = [
    ("Cargo.toml", "[package]\nname = \"borg\"\n"),
    ("Cargo.lock", "version = 4\n"),
    ("README.md", "# borg\n\nResistance is futile.\n"),
    ("LICENSE", "MIT\n"),
    ("Dockerfile", "FROM rust:1\n"),
    ("package.json", "{ \"name\": \"borg-web\" }\n"),
    ("tsconfig.json", "{}\n"),
    ("next.config.js", "module.exports = {};\n"),
    ("notes.txt", "We are the Borg.\nLower your shields and surrender your files.\n"),
    ("docs/guide.md", "# Guide\n"),
    ("docs/architecture.md", "# Architecture\n"),
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

fn project(fixture: &Fixture) -> Result<PathBuf> {
    let root = PROJECT.iter().fold(fixture.home(), |path, part| path.join(part));
    SOURCES.iter().chain(&FILES).try_for_each(|(path, body)| write(&root.join(path), body.as_bytes()))?;
    let script = root.join(SCRIPT);
    write(&script, SCRIPT_BODY.as_bytes())?;
    fs::set_permissions(&script, fs::Permissions::from_mode(EXECUTABLE))?;
    for (name, seed) in PICTURES {
        write(&root.join(ASSETS).join(name), &picture(seed)?)?;
    }
    fixture.configure(CONFIG)?;
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

    fn find(&mut self, query: &str) -> Result<&mut Self> {
        self.press(keys::ctrl('s'), SHORT)?.type_text(query, SHORT)?.press(keys::enter(), SHORT)
    }
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let binary = fs::canonicalize(PathBuf::from(args.next().context(USAGE)?))?;
    let output = PathBuf::from(args.next().context(USAGE)?);

    let fixture = Fixture::new()?;
    let root = project(&fixture)?;
    let app = Session::builder(&binary, &fixture).cwd(root).size(COLS, ROWS).label("demo").spawn()?;
    let mut script = Script { app, recorder: Recorder::new(FRAME) };
    script.recorder.record(&mut script.app, SPLASH)?;

    for _ in 0..4 {
        script.press(keys::down(), SHORT)?;
    }
    script
        .press(keys::ctrl('s'), SHORT)?
        .type_text("js", BEAT)?
        .press(keys::down(), BEAT)?
        .press(keys::enter(), SHORT)?;

    script.find("src")?.press(keys::enter(), LONG)?;
    script.press(keys::key(KeyCode::Home), SHORT)?.press(keys::enter(), LONG)?;

    script.type_text("ls -la", SHORT)?.press(keys::enter(), LONG)?.press(keys::ctrl('o'), LONG)?;

    script.find("README")?;
    for _ in 0..3 {
        script.press(keys::key(KeyCode::Insert), SHORT)?;
    }
    script.press(keys::f(9), BEAT)?.press(keys::right(), SHORT)?.press(keys::down(), SHORT)?;
    script.press(keys::down(), SHORT)?.press(keys::esc(), BEAT)?;

    script.press(keys::f(9), SHORT)?;
    for _ in 0..3 {
        script.press(keys::right(), SHORT)?;
    }
    script.press(keys::enter(), BEAT)?;
    for key in [keys::up(), keys::down(), keys::down(), keys::down()] {
        script.press(key, LONG)?;
    }
    script.press(keys::esc(), BEAT)?;

    script.find("assets")?.press(keys::enter(), BEAT)?.find("sunset")?;
    script.press(keys::f(3), LONG)?.press(keys::right(), LONG)?.press(keys::right(), LONG)?.press(keys::esc(), BEAT)?;

    script.press(keys::f(1), LONG)?.type_text("borg", LONG)?.press(keys::esc(), BEAT)?;

    let frames = script.recorder.frames();
    script.recorder.save_gif(&output, TAIL)?;
    println!("wrote {frames} frames to {}", output.display());
    Ok(())
}
