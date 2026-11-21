use crate::Refusal;
use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAError, LAPolicy};
use std::sync::mpsc;

const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthentication;

pub fn confirm_owner(reason: &str) -> Result<(), Refusal> {
    let context = unsafe { LAContext::new() };
    unsafe { context.canEvaluatePolicy_error(POLICY) }.map_err(|error| refusal(&error))?;
    let (sender, answer) = mpsc::channel();
    let reply = RcBlock::new(move |confirmed: Bool, error: *mut NSError| {
        let outcome = match unsafe { error.as_ref() } {
            _ if confirmed.as_bool() => Ok(()),
            Some(error) => Err(refusal(error)),
            None => Err(Refusal::Declined(String::new())),
        };
        sender.send(outcome).ok();
    });
    unsafe { context.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &reply) };
    answer.recv().unwrap_or_else(|_| Err(Refusal::Declined(String::new())))
}

fn refusal(error: &NSError) -> Refusal {
    if error.code() == LAError::NotInteractive.0 {
        Refusal::NoScreen
    } else {
        Refusal::Declined(error.localizedDescription().to_string())
    }
}
