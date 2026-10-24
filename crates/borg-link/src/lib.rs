//! Borg Link: an end-to-end encrypted channel between borg on a server and borg on the
//! user's computer, through a relay both reach with outgoing connections.
//!
//! Each borg holds a static X25519 key. A computer pairs with a server once, through a
//! one-time link the server shows and approves; from then on each knows the other's key.
//! Every channel is a Noise IK handshake bound to those keys, so the relay, or anyone on
//! the network, forwards ciphertext it can neither read, change nor answer in either side's
//! name.

mod channel;
mod key;
mod link;
pub mod relay;

pub use channel::{Channel, MESSAGE_LIMIT, Socket, initiate, respond};
pub use key::{Identity, KEY_LEN, PublicKey, fingerprint, hex, unhex};
pub use link::{PairingLink, Relay, SECRET_LEN, secret};
