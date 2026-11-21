use aes::Aes256;
use aes::cipher::{Block, BlockEncrypt, KeyInit};
use blowfish::Blowfish;
use cmac::{Cmac, Mac};
use subtle::ConstantTimeEq;

const NONCE_TWEAK: u8 = 0;
const HEADER_TWEAK: u8 = 1;
const MESSAGE_TWEAK: u8 = 2;

#[derive(Debug)]
pub(super) struct TagMismatch;

pub(super) trait EaxCipher: BlockEncrypt + KeyInit {
    type Omac: Mac + KeyInit;
}

impl EaxCipher for Aes256 {
    type Omac = Cmac<Aes256>;
}

impl EaxCipher for Blowfish {
    type Omac = Cmac<Blowfish>;
}

#[cfg(test)]
impl EaxCipher for aes::Aes128 {
    type Omac = Cmac<aes::Aes128>;
}

pub(super) fn seal<C>(key: &[u8], nonce: &[u8], aad: &[u8], buffer: &mut [u8]) -> Vec<u8>
where
    C: EaxCipher,
{
    let start = omac::<C>(key, NONCE_TWEAK, nonce);
    apply_keystream(&cipher::<C>(key), &start, buffer);
    tag::<C>(key, &start, aad, buffer)
}

pub(super) fn open<C>(key: &[u8], nonce: &[u8], aad: &[u8], buffer: &mut [u8], tag: &[u8]) -> Result<(), TagMismatch>
where
    C: EaxCipher,
{
    let start = omac::<C>(key, NONCE_TWEAK, nonce);
    let expected = self::tag::<C>(key, &start, aad, buffer);
    if !bool::from(expected.as_slice().ct_eq(tag)) {
        return Err(TagMismatch);
    }
    apply_keystream(&cipher::<C>(key), &start, buffer);
    Ok(())
}

fn cipher<C: KeyInit>(key: &[u8]) -> C {
    C::new_from_slice(key).expect("the key length suits the cipher")
}

fn tag<C>(key: &[u8], start: &[u8], aad: &[u8], ciphertext: &[u8]) -> Vec<u8>
where
    C: EaxCipher,
{
    let header = omac::<C>(key, HEADER_TWEAK, aad);
    let message = omac::<C>(key, MESSAGE_TWEAK, ciphertext);
    start.iter().zip(&header).zip(&message).map(|((start, header), message)| start ^ header ^ message).collect()
}

fn omac<C>(key: &[u8], tweak: u8, data: &[u8]) -> Vec<u8>
where
    C: EaxCipher,
{
    let mut first = vec![0; C::block_size()];
    if let Some(last) = first.last_mut() {
        *last = tweak;
    }
    let mut mac = <C::Omac as KeyInit>::new_from_slice(key).expect("the key length suits the cipher");
    mac.update(&first);
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn apply_keystream<C: BlockEncrypt>(cipher: &C, start: &[u8], buffer: &mut [u8]) {
    let mut counter = Block::<C>::clone_from_slice(start);
    for piece in buffer.chunks_mut(C::block_size()) {
        let mut stream = counter.clone();
        cipher.encrypt_block(&mut stream);
        piece.iter_mut().zip(stream.iter()).for_each(|(byte, key)| *byte ^= key);
        increment(&mut counter);
    }
}

fn increment(counter: &mut [u8]) {
    for byte in counter.iter_mut().rev() {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            break;
        }
    }
}
