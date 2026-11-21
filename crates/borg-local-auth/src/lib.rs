#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    NoScreen,
    Declined(String),
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::confirm_owner;
