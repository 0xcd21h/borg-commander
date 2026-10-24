//! The pairing code: six emoji, each with its name, that both ends of a pairing show for the
//! user to compare. It comes from the handshake the two just finished, so it matches only when
//! they share one session. A borg that used the link first has a session of its own, with a
//! code it cannot choose, since the server's half of each handshake is new.

/// Symbols in a code, six bits each.
const SYMBOLS: usize = 6;
const BITS: u32 = 6;

/// Emoji that terminals draw two columns wide, with names that are told apart easily.
const EMOJI: [(&str, &str); 64] = [
    ("🐶", "Dog"),
    ("🐱", "Cat"),
    ("🦁", "Lion"),
    ("🐎", "Horse"),
    ("🦄", "Unicorn"),
    ("🐷", "Pig"),
    ("🐘", "Elephant"),
    ("🐰", "Rabbit"),
    ("🐼", "Panda"),
    ("🐓", "Rooster"),
    ("🐧", "Penguin"),
    ("🐢", "Turtle"),
    ("🐟", "Fish"),
    ("🐙", "Octopus"),
    ("🦋", "Butterfly"),
    ("🌷", "Tulip"),
    ("🌳", "Tree"),
    ("🌵", "Cactus"),
    ("🍄", "Mushroom"),
    ("🌏", "Globe"),
    ("🌙", "Moon"),
    ("🌈", "Rainbow"),
    ("🔥", "Fire"),
    ("🍌", "Banana"),
    ("🍎", "Apple"),
    ("🍓", "Strawberry"),
    ("🌽", "Corn"),
    ("🍕", "Pizza"),
    ("🎂", "Cake"),
    ("🎈", "Balloon"),
    ("😀", "Smiley"),
    ("🤖", "Robot"),
    ("🎩", "Hat"),
    ("👓", "Glasses"),
    ("🔧", "Wrench"),
    ("🎅", "Santa"),
    ("👍", "Thumb"),
    ("🌂", "Umbrella"),
    ("⌛", "Hourglass"),
    ("⏰", "Clock"),
    ("🎁", "Gift"),
    ("💡", "Bulb"),
    ("📕", "Book"),
    ("📏", "Ruler"),
    ("📎", "Paperclip"),
    ("🎲", "Dice"),
    ("🔒", "Lock"),
    ("🔑", "Key"),
    ("🔨", "Hammer"),
    ("📞", "Phone"),
    ("🏁", "Flag"),
    ("🚂", "Train"),
    ("🚲", "Bicycle"),
    ("🚁", "Helicopter"),
    ("🚀", "Rocket"),
    ("🏆", "Trophy"),
    ("⚽", "Ball"),
    ("🎸", "Guitar"),
    ("🎺", "Trumpet"),
    ("🔔", "Bell"),
    ("⚓", "Anchor"),
    ("🎧", "Headphones"),
    ("📁", "Folder"),
    ("📌", "Pin"),
];

/// A session's pairing code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode([(&'static str, &'static str); SYMBOLS]);

impl PairingCode {
    /// The code for the hash of a finished handshake.
    pub fn of(handshake_hash: &[u8]) -> Self {
        let mut first = [0; 8];
        first.iter_mut().zip(handshake_hash).for_each(|(byte, hashed)| *byte = *hashed);
        let bits = u64::from_be_bytes(first);
        Self(std::array::from_fn(|n| EMOJI[(bits >> (64 - BITS * (n as u32 + 1))) as usize % EMOJI.len()]))
    }

    /// The code in two rows of three, as both ends show it: with each emoji before its name,
    /// or the names alone, for a terminal that draws emoji narrower than they are laid out.
    pub fn rows(&self, emoji: bool) -> [String; 2] {
        let row = |symbols: &[(&str, &str)]| {
            let shown = symbols
                .iter()
                .map(|(symbol, name)| if emoji { format!("{symbol} {name}") } else { (*name).to_owned() });
            shown.collect::<Vec<_>>().join(" · ")
        };
        [row(&self.0[..SYMBOLS / 2]), row(&self.0[SYMBOLS / 2..])]
    }

    /// The names alone, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|(_, name)| *name).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use unicode_width::UnicodeWidthStr;

    use super::*;

    #[test]
    fn every_symbol_is_two_columns_wide_and_named_once() {
        for (emoji, name) in EMOJI {
            assert_eq!(emoji.width(), 2, "{emoji} {name}");
            assert_eq!(emoji.chars().count(), 1, "{name} needs no variation selector");
        }
        assert_eq!(EMOJI.iter().map(|(emoji, _)| emoji).collect::<HashSet<_>>().len(), EMOJI.len());
        assert_eq!(EMOJI.iter().map(|(_, name)| name.to_lowercase()).collect::<HashSet<_>>().len(), EMOJI.len());
    }

    #[test]
    fn a_code_takes_six_bits_per_symbol_from_the_start_of_the_hash() {
        // 000000 000001 000010 000011 111111 111110, then bits the code leaves out.
        let hash = [0b0000_0000, 0b0001_0000, 0b1000_0011, 0b1111_1111, 0b1110_0000, 0xff, 0xff, 0xff];
        let code = PairingCode::of(&hash);
        assert_eq!(code.names(), ["Dog", "Cat", "Lion", "Horse", "Pin", "Folder"]);
        assert_eq!(
            code.rows(true),
            ["🐶 Dog · 🐱 Cat · 🦁 Lion".to_owned(), "🐎 Horse · 📌 Pin · 📁 Folder".to_owned()]
        );
        assert_eq!(code.rows(false), ["Dog · Cat · Lion".to_owned(), "Horse · Pin · Folder".to_owned()]);
        assert_ne!(PairingCode::of(&[1; 32]), PairingCode::of(&[2; 32]));
    }
}
