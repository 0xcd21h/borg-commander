use anyhow::{Result, anyhow};
use borg_i18n::tr;
use borg_local_auth::Refusal;
use security_framework::passwords::{PasswordOptions, generic_password, set_generic_password_options};
use std::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "Borg Commander";
pub(super) const ACCOUNT: &str = "computer key";
const NOT_FOUND: i32 = -25300;

fn item(account: &str) -> PasswordOptions {
    PasswordOptions::new_generic_password(SERVICE, account)
}

fn failed(error: impl std::fmt::Display) -> anyhow::Error {
    anyhow!(tr!(cloud_key_keychain_failed, reason = error.to_string()))
}

fn confirm_owner() -> Result<()> {
    static CONFIRMED: Mutex<bool> = Mutex::new(false);
    let mut confirmed = CONFIRMED.lock().expect("owner confirmation");
    if !*confirmed {
        borg_local_auth::confirm_owner(tr!(cloud_key_confirm_reason)).map_err(|refusal| match refusal {
            Refusal::NoScreen => anyhow!(tr!(cloud_key_no_screen)),
            Refusal::Declined(reason) => anyhow!(tr!(cloud_key_not_confirmed, reason = reason)),
        })?;
        *confirmed = true;
    }
    Ok(())
}

pub(super) fn read(account: &str) -> Result<Option<Zeroizing<String>>> {
    confirm_owner()?;
    match generic_password(item(account)) {
        Ok(bytes) => String::from_utf8(bytes).map(|text| Some(Zeroizing::new(text))).map_err(|error| {
            error.into_bytes().zeroize();
            failed(tr!(cloud_key_keychain_garbled))
        }),
        Err(error) if error.code() == NOT_FOUND => Ok(None),
        Err(error) => Err(failed(error)),
    }
}

pub(super) fn write(account: &str, text: &str) -> Result<()> {
    set_generic_password_options(text.as_bytes(), item(account)).map_err(failed)
}

#[cfg(test)]
pub(super) fn delete(account: &str) -> Result<()> {
    match security_framework::passwords::delete_generic_password_options(item(account)) {
        Err(error) if error.code() != NOT_FOUND => Err(failed(error)),
        _ => Ok(()),
    }
}
