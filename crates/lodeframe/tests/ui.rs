// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Misuse of the derives must fail to compile with a message that points at the cause.

#[test]
fn misuse_fails_to_compile() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
