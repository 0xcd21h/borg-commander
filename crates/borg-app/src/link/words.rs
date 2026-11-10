use anyhow::{Result, anyhow};

const WORDS: &str = include_str!("words.txt");
pub(super) const SUGGESTED: usize = 4;

pub(super) fn list() -> Vec<&'static str> {
    WORDS.lines().collect()
}

pub(super) fn suggestion() -> Result<String> {
    let words = list();
    let mut random = [0; SUGGESTED * 2];
    getrandom::fill(&mut random).map_err(|error| anyhow!("{error}"))?;
    let chosen: Vec<&str> =
        random.chunks(2).map(|pair| words[usize::from(u16::from_be_bytes([pair[0], pair[1]])) % words.len()]).collect();
    Ok(chosen.join(" "))
}
