use sha2::{Digest, Sha256};
use std::fmt::Write as _;

#[test]
fn committed_valid_block_fixture_uses_the_independent_known_body_digest() {
    let fixture = include_bytes!("fixtures/managed-files/agents/valid-block.md");
    let body = b"## Agent Lowmem resource policy\n\nknown body\n";
    let expected = "00d2a098ba3ab0524961bc97197d5a12a73591e102a0055f9df0f6a09f2ddb55";
    let actual = Sha256::digest(body)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        });

    assert_eq!(actual, expected);
    assert!(
        fixture
            .windows(expected.len())
            .any(|window| window == expected.as_bytes())
    );
}
