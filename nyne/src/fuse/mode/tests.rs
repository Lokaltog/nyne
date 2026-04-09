use rstest::rstest;

use super::{EXECUTE_BITS, PermissionsExt, READ_BITS, WRITE_BITS};
use crate::router::Permissions;

/// `Permissions → FUSE u16 mode bits` translation covers every capability
/// combination plus the empty and all-set extremes.
#[rstest]
#[case::none(Permissions::NONE, 0)]
#[case::read_only(Permissions::READ, READ_BITS)]
#[case::write_only(Permissions::WRITE, WRITE_BITS)]
#[case::execute_only(Permissions::EXECUTE, EXECUTE_BITS)]
#[case::read_write(Permissions::READ | Permissions::WRITE, READ_BITS | WRITE_BITS)]
#[case::read_execute(Permissions::READ | Permissions::EXECUTE, READ_BITS | EXECUTE_BITS)]
#[case::write_execute(Permissions::WRITE | Permissions::EXECUTE, WRITE_BITS | EXECUTE_BITS)]
#[case::all(Permissions::ALL, READ_BITS | WRITE_BITS | EXECUTE_BITS)]
fn permissions_to_mode_bits(#[case] perms: Permissions, #[case] expected: u16) {
    assert_eq!(perms.to_mode_bits(), expected);
}

/// All mapped bit constants live in the owner triple — the VFS is single-user,
/// so group/other bits must always be zero.
#[rstest]
#[case::read(READ_BITS)]
#[case::write(WRITE_BITS)]
#[case::execute(EXECUTE_BITS)]
fn bits_are_owner_only(#[case] bits: u16) {
    assert_eq!(bits & 0o077, 0, "bit {bits:o} leaks into group/other");
}
