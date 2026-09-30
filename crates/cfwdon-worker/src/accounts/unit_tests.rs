use crate::accounts::{DirectoryOrder, directory_order};

#[test]
fn directory_order_defaults_to_active_and_accepts_new() {
    assert_eq!(directory_order(None), DirectoryOrder::Active);
    assert_eq!(directory_order(Some("active")), DirectoryOrder::Active);
    assert_eq!(directory_order(Some("new")), DirectoryOrder::New);
    assert_eq!(directory_order(Some("NEW")), DirectoryOrder::New);
    assert_eq!(directory_order(Some("unexpected")), DirectoryOrder::Active);
}
