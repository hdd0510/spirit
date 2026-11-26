use std::path::Path;

use super::*;

#[test]
fn commands_sharing_a_control_master_share_a_bounded_pool() {
    let first = control_master_slots(Path::new("/tmp/spirit-test-master-a"));
    let again = control_master_slots(Path::new("/tmp/spirit-test-master-a"));
    let other = control_master_slots(Path::new("/tmp/spirit-test-master-b"));

    assert!(Arc::ptr_eq(&first, &again));
    assert!(!Arc::ptr_eq(&first, &other));

    let held: Vec<_> = (0..MAX_COMMANDS_PER_CONTROL_MASTER)
        .map(|_| first.try_acquire().expect("slot available"))
        .collect();
    assert!(again.try_acquire().is_err());
    assert!(other.try_acquire().is_ok());
    drop(held);
    assert!(again.try_acquire().is_ok());
}
