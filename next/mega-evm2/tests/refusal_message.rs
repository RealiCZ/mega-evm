//! A refusal Satin has no evm2 error variant for carries its text: the block executor reports a
//! refused transaction by the error's display, and a node shows it to the sender.

use mega_evm2::satin_ext::{Refusal, refuse};

/// The text of the refusal is what the error displays, whole.
#[test]
fn test_a_refusal_displays_its_text() {
    let text =
        "a transaction from the system address must be a legacy call to a whitelisted contract";
    assert_eq!(refuse(text).to_string(), text);
    assert_eq!(Refusal("another reason").to_string(), "another reason");
}

/// Two refusals with different texts display differently.
#[test]
fn test_refusals_are_told_apart_by_their_text() {
    assert_ne!(refuse("one").to_string(), refuse("two").to_string());
    assert!(!refuse("one").to_string().is_empty());
}
