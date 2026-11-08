//! The per-transaction state of the mechanisms is reset between the transactions of a block: the
//! Oracle hints of one transaction are not the next one's, and the hint list keeps its buffer.

use alloy_primitives::{Address, B256, Bytes};
use mega_evm2::satin_ext::MechanismState;

#[test]
fn test_reset_empties_the_hints_and_keeps_their_buffer() {
    let mut state = MechanismState::default();
    for i in 0..5u8 {
        state.hints.push((
            Address::with_last_byte(i),
            B256::with_last_byte(i),
            Bytes::from(vec![i]),
        ));
    }
    state.hint_bytes = 99;
    state.disabled_from = Some(2);
    let capacity = state.hints.capacity();
    assert!(capacity >= 5);
    state.reset();
    assert!(state.hints.is_empty());
    assert_eq!(state.hint_bytes, 0);
    assert_eq!(state.disabled_from, None);
    // The buffer is reused by the next transaction, not allocated again.
    assert_eq!(state.hints.capacity(), capacity);
}
