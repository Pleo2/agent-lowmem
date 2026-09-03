// These primitives are wired into managed-file planning in Task 8.
#![allow(dead_code)]

use crate::{
    configuration::{AgentLowmemConfig, valid_key},
    result::Reason,
};
use sha2::{Digest, Sha256};
use std::{fmt, fmt::Write as _, ops::Range};

pub(crate) const MAX_AGENTS_BYTES: usize = 1_048_576;
pub(crate) const MAX_MANAGED_BLOCK_BYTES: usize = 65_536;

const START_TOKEN: &[u8] = b"<!-- agent-lowmem:start";
const END_TOKEN: &[u8] = b"<!-- agent-lowmem:end";
const START_PREFIX: &str = "<!-- agent-lowmem:start format=\"1\" content-sha256=\"";
const START_SUFFIX: &str = "\" -->";
const END_MARKER: &[u8] = b"<!-- agent-lowmem:end -->";
const POLICY_PREAMBLE: &str = "## Agent Lowmem resource policy\n\n\
Run supported heavy validation through Agent Lowmem. Run only one heavy\n\
operation at a time, never use watch mode, and prefer focused validation\n\
before broad suites. Do not retry OOM or timeout failures automatically.\n\
Agent Lowmem v1 does not impose a memory cap or guarantee responsiveness;\n\
use CI when a broad build cannot be constrained locally.\n\n\
Supported commands:\n";

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AgentsDocumentState {
    Absent,
    NoBlock { bytes: Vec<u8> },
    OneBlock(ManagedBlock),
}

impl fmt::Debug for AgentsDocumentState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("AgentsDocumentState::Absent"),
            Self::NoBlock { bytes } => formatter
                .debug_struct("AgentsDocumentState::NoBlock")
                .field("byte_length", &bytes.len())
                .finish(),
            Self::OneBlock(block) => formatter
                .debug_tuple("AgentsDocumentState::OneBlock")
                .field(block)
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ManagedBlock {
    pub span: Range<usize>,
    pub body: Range<usize>,
    pub format: u8,
    pub declared_sha256: [u8; 32],
    document: Vec<u8>,
}

impl fmt::Debug for ManagedBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedBlock")
            .field("span", &self.span)
            .field("body", &self.body)
            .field("format", &self.format)
            .field("declared_sha256", &hex_digest(&self.declared_sha256))
            .field("document_byte_length", &self.document.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AgentsEdit {
    pub target_bytes: Vec<u8>,
    pub managed_span: Range<usize>,
    pub inserted_separator: Vec<u8>,
}

impl fmt::Debug for AgentsEdit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentsEdit")
            .field("target_byte_length", &self.target_bytes.len())
            .field("managed_span", &self.managed_span)
            .field("separator_byte_length", &self.inserted_separator.len())
            .finish()
    }
}

pub(crate) fn inspect_agents(bytes: Option<Vec<u8>>) -> Result<AgentsDocumentState, Reason> {
    let Some(bytes) = bytes else {
        return Ok(AgentsDocumentState::Absent);
    };
    if bytes.len() > MAX_AGENTS_BYTES || std::str::from_utf8(&bytes).is_err() {
        return Err(Reason::ManagedFileConflict);
    }

    let starts = marker_positions(&bytes, START_TOKEN);
    let ends = marker_positions(&bytes, END_TOKEN);
    if starts.is_empty() && ends.is_empty() {
        return Ok(AgentsDocumentState::NoBlock { bytes });
    }
    if starts.len() != 1 || ends.len() != 1 {
        return Err(Reason::ManagedFileConflict);
    }

    let start = starts[0];
    let end = ends[0];
    if start >= end
        || !at_line_start(&bytes, start)
        || !at_line_start(&bytes, end)
        || !bytes[end..].starts_with(END_MARKER)
    {
        return Err(Reason::ManagedFileConflict);
    }
    let start_line_end = bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| start + offset)
        .ok_or(Reason::ManagedFileConflict)?;
    let start_line = std::str::from_utf8(&bytes[start..start_line_end])
        .map_err(|_| Reason::ManagedFileConflict)?;
    let digest_text = start_line
        .strip_prefix(START_PREFIX)
        .and_then(|remaining| remaining.strip_suffix(START_SUFFIX))
        .ok_or(Reason::ManagedFileConflict)?;
    let declared_sha256 = parse_lower_sha256(digest_text)?;

    let body = start_line_end + 1..end;
    if end > start_line_end + 1 && bytes[end - 1] != b'\n' {
        return Err(Reason::ManagedFileConflict);
    }
    let actual_sha256: [u8; 32] = Sha256::digest(&bytes[body.clone()]).into();
    if actual_sha256 != declared_sha256 {
        return Err(Reason::ManagedFileConflict);
    }

    let marker_end = end + END_MARKER.len();
    let span_end = match bytes.get(marker_end) {
        None => marker_end,
        Some(b'\n') => marker_end + 1,
        Some(_) => return Err(Reason::ManagedFileConflict),
    };
    Ok(AgentsDocumentState::OneBlock(ManagedBlock {
        span: start..span_end,
        body,
        format: 1,
        declared_sha256,
        document: bytes,
    }))
}

pub(crate) fn render_policy_body(config: &AgentLowmemConfig) -> Result<Vec<u8>, Reason> {
    if !config.has_operations() {
        return Err(Reason::OperationUnsupported);
    }
    let mut body = String::from(POLICY_PREAMBLE);
    for operation_key in config.operations.keys() {
        if !valid_key(operation_key) {
            return Err(Reason::InvalidConfig);
        }
        writeln!(body, "- `agent-lowmem run {operation_key}`")
            .map_err(|_| Reason::InternalError)?;
    }
    for (workspace_key, workspace) in &config.workspaces {
        if !valid_key(workspace_key) {
            return Err(Reason::InvalidConfig);
        }
        for operation_key in workspace.operations.keys() {
            if !valid_key(operation_key) {
                return Err(Reason::InvalidConfig);
            }
            writeln!(
                body,
                "- `agent-lowmem run {operation_key} --workspace {workspace_key}`"
            )
            .map_err(|_| Reason::InternalError)?;
        }
    }
    Ok(body.into_bytes())
}

pub(crate) fn plan_agents_edit(
    current: AgentsDocumentState,
    body: &[u8],
) -> Result<AgentsEdit, Reason> {
    if std::str::from_utf8(body).is_err() || !body.ends_with(b"\n") {
        return Err(Reason::ManagedFileConflict);
    }
    let block = render_block(body);
    if block.len() > MAX_MANAGED_BLOCK_BYTES {
        return Err(Reason::ManagedFileConflict);
    }

    let (mut target_bytes, managed_start, inserted_separator) = match current {
        AgentsDocumentState::Absent => (Vec::new(), 0, Vec::new()),
        AgentsDocumentState::NoBlock { bytes } if bytes.is_empty() => (bytes, 0, Vec::new()),
        AgentsDocumentState::NoBlock { mut bytes } => {
            let separator = if bytes.ends_with(b"\n\n") {
                Vec::new()
            } else if bytes.ends_with(b"\n") {
                b"\n".to_vec()
            } else {
                b"\n\n".to_vec()
            };
            let managed_start = bytes.len();
            bytes.extend_from_slice(&separator);
            (bytes, managed_start, separator)
        }
        AgentsDocumentState::OneBlock(managed) => {
            let mut target =
                Vec::with_capacity(managed.document.len() - managed.span.len() + block.len());
            target.extend_from_slice(&managed.document[..managed.span.start]);
            let managed_start = target.len();
            target.extend_from_slice(&block);
            target.extend_from_slice(&managed.document[managed.span.end..]);
            if target.len() > MAX_AGENTS_BYTES {
                return Err(Reason::ManagedFileConflict);
            }
            return Ok(AgentsEdit {
                target_bytes: target,
                managed_span: managed_start..managed_start + block.len(),
                inserted_separator: Vec::new(),
            });
        }
    };

    target_bytes.extend_from_slice(&block);
    if target_bytes.len() > MAX_AGENTS_BYTES {
        return Err(Reason::ManagedFileConflict);
    }
    Ok(AgentsEdit {
        managed_span: managed_start..target_bytes.len(),
        target_bytes,
        inserted_separator,
    })
}

fn render_block(body: &[u8]) -> Vec<u8> {
    let digest: [u8; 32] = Sha256::digest(body).into();
    let start = format!("{START_PREFIX}{}{START_SUFFIX}\n", hex_digest(&digest));
    let mut block = Vec::with_capacity(start.len() + body.len() + END_MARKER.len() + 1);
    block.extend_from_slice(start.as_bytes());
    block.extend_from_slice(body);
    block.extend_from_slice(END_MARKER);
    block.push(b'\n');
    block
}

fn marker_positions(bytes: &[u8], marker: &[u8]) -> Vec<usize> {
    bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(|(index, window)| (window == marker).then_some(index))
        .collect()
}

fn at_line_start(bytes: &[u8], index: usize) -> bool {
    index == 0 || bytes.get(index - 1) == Some(&b'\n')
}

fn parse_lower_sha256(value: &str) -> Result<[u8; 32], Reason> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Reason::ManagedFileConflict);
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(digest)
}

fn hex_nibble(byte: u8) -> Result<u8, Reason> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Reason::ManagedFileConflict),
    }
}

fn hex_digest(digest: &[u8; 32]) -> String {
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::{
        AgentsDocumentState, MAX_AGENTS_BYTES, MAX_MANAGED_BLOCK_BYTES, inspect_agents,
        plan_agents_edit, render_policy_body,
    };
    use crate::{
        configuration::{AgentLowmemConfig, OperationConfig, WorkspaceConfig},
        repository::PackageManagerKind,
        result::Reason,
    };
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, fmt::Write as _};

    const KNOWN_BODY: &[u8] = b"## Agent Lowmem resource policy\n\nknown body\n";
    const KNOWN_DIGEST: &str = "00d2a098ba3ab0524961bc97197d5a12a73591e102a0055f9df0f6a09f2ddb55";

    fn operation(script: &str, timeout_seconds: u16) -> OperationConfig {
        OperationConfig {
            script: script.to_owned(),
            timeout_seconds,
        }
    }

    fn test_hex(bytes: &[u8]) -> String {
        bytes.iter().fold(
            String::with_capacity(bytes.len() * 2),
            |mut output, byte| {
                write!(output, "{byte:02x}").expect("writing to a String cannot fail");
                output
            },
        )
    }

    fn test_config() -> AgentLowmemConfig {
        AgentLowmemConfig {
            version: 1,
            package_manager: PackageManagerKind::Pnpm,
            operations: BTreeMap::from([
                ("test".to_owned(), operation("private-test-script", 900)),
                ("build".to_owned(), operation("private-build-script", 1_800)),
            ]),
            workspaces: BTreeMap::from([
                (
                    "web".to_owned(),
                    WorkspaceConfig {
                        path: "apps/private-web-path".to_owned(),
                        package_name: "@private/web-package".to_owned(),
                        operations: BTreeMap::from([(
                            "typecheck".to_owned(),
                            operation("private-typecheck-script", 900),
                        )]),
                    },
                ),
                (
                    "api".to_owned(),
                    WorkspaceConfig {
                        path: "apps/private-api-path".to_owned(),
                        package_name: "@private/api-package".to_owned(),
                        operations: BTreeMap::from([(
                            "lint".to_owned(),
                            operation("private-lint-script", 900),
                        )]),
                    },
                ),
            ]),
        }
    }

    fn block(body: &[u8]) -> Vec<u8> {
        let digest = test_hex(&Sha256::digest(body));
        format!(
            "<!-- agent-lowmem:start format=\"1\" content-sha256=\"{digest}\" -->\n{}<!-- agent-lowmem:end -->\n",
            String::from_utf8(body.to_vec()).unwrap()
        )
        .into_bytes()
    }

    #[test]
    fn scans_absent_unmanaged_and_one_independently_hashed_block() {
        assert!(matches!(
            inspect_agents(None).unwrap(),
            AgentsDocumentState::Absent
        ));
        let unmanaged = b"# Existing instructions\n".to_vec();
        assert!(matches!(
            inspect_agents(Some(unmanaged.clone())).unwrap(),
            AgentsDocumentState::NoBlock { bytes } if bytes == unmanaged
        ));

        let valid = include_bytes!("../tests/fixtures/managed-files/agents/valid-block.md");
        let state = inspect_agents(Some(valid.to_vec())).unwrap();
        let AgentsDocumentState::OneBlock(managed) = state else {
            panic!("expected one managed block");
        };
        assert_eq!(&valid[managed.body.clone()], KNOWN_BODY);
        assert_eq!(managed.format, 1);
        assert_eq!(
            managed.declared_sha256,
            <[u8; 32]>::try_from(Sha256::digest(KNOWN_BODY).as_slice()).unwrap()
        );
        assert_eq!(&valid[managed.span], valid);
        let actual_digest = test_hex(&Sha256::digest(KNOWN_BODY));
        assert_eq!(KNOWN_DIGEST, actual_digest);
    }

    #[test]
    fn rejects_every_ambiguous_or_invalid_marker_state() {
        let valid = block(KNOWN_BODY);
        let uppercase = String::from_utf8(valid.clone())
            .unwrap()
            .replace(KNOWN_DIGEST, &KNOWN_DIGEST.to_ascii_uppercase())
            .into_bytes();
        let invalid_digest = String::from_utf8(valid.clone())
            .unwrap()
            .replace(KNOWN_DIGEST, "not-a-digest")
            .into_bytes();
        let mismatch = String::from_utf8(valid.clone())
            .unwrap()
            .replace("known body", "wrong body")
            .into_bytes();
        let unsupported = String::from_utf8(valid.clone())
            .unwrap()
            .replace("format=\"1\"", "format=\"2\"")
            .into_bytes();
        let start =
            format!("<!-- agent-lowmem:start format=\"1\" content-sha256=\"{KNOWN_DIGEST}\" -->\n");
        let end = "<!-- agent-lowmem:end -->\n";
        let nested = format!("{start}{start}known body\n{end}{end}").into_bytes();

        for bytes in [
            [valid.clone(), valid].concat(),
            nested,
            start.clone().into_bytes(),
            end.as_bytes().to_vec(),
            unsupported,
            uppercase,
            invalid_digest,
            mismatch,
            b"prefix <!-- agent-lowmem:start suffix".to_vec(),
            b"<!-- agent-lowmem:end".to_vec(),
            b"<!-- agent-lowmem:end malformed -->".to_vec(),
            vec![0xff, 0xfe],
            vec![b'x'; MAX_AGENTS_BYTES + 1],
        ] {
            assert_eq!(
                inspect_agents(Some(bytes)).unwrap_err(),
                Reason::ManagedFileConflict
            );
        }
    }

    #[test]
    fn renders_only_sorted_operation_identities_in_the_exact_v1_body() {
        let body = render_policy_body(&test_config()).unwrap();

        assert_eq!(
            body,
            b"## Agent Lowmem resource policy\n\nRun supported heavy validation through Agent Lowmem. Run only one heavy\noperation at a time, never use watch mode, and prefer focused validation\nbefore broad suites. Do not retry OOM or timeout failures automatically.\nAgent Lowmem v1 does not impose a memory cap or guarantee responsiveness;\nuse CI when a broad build cannot be constrained locally.\n\nSupported commands:\n- `agent-lowmem run build`\n- `agent-lowmem run test`\n- `agent-lowmem run lint --workspace api`\n- `agent-lowmem run typecheck --workspace web`\n"
        );
        let rendered = String::from_utf8(body).unwrap();
        for secret in [
            "private-test-script",
            "private-build-script",
            "private-typecheck-script",
            "private-lint-script",
            "private-web-path",
            "private-api-path",
            "@private/web-package",
            "@private/api-package",
            "<operation>",
            "<workspace>",
        ] {
            assert!(!rendered.contains(secret));
        }
    }

    #[test]
    fn places_new_blocks_with_only_the_required_owned_separator() {
        let desired = block(KNOWN_BODY);
        for (state, expected_prefix, expected_separator) in [
            (AgentsDocumentState::Absent, b"".as_slice(), b"".as_slice()),
            (
                AgentsDocumentState::NoBlock { bytes: Vec::new() },
                b"".as_slice(),
                b"".as_slice(),
            ),
            (
                AgentsDocumentState::NoBlock {
                    bytes: b"# Existing\n\n".to_vec(),
                },
                b"# Existing\n\n".as_slice(),
                b"".as_slice(),
            ),
            (
                AgentsDocumentState::NoBlock {
                    bytes: b"# Existing\n".to_vec(),
                },
                b"# Existing\n".as_slice(),
                b"\n".as_slice(),
            ),
            (
                AgentsDocumentState::NoBlock {
                    bytes: b"# Existing".to_vec(),
                },
                b"# Existing".as_slice(),
                b"\n\n".as_slice(),
            ),
        ] {
            let edit = plan_agents_edit(state, KNOWN_BODY).unwrap();
            assert_eq!(edit.inserted_separator, expected_separator);
            assert_eq!(
                edit.target_bytes,
                [expected_prefix, expected_separator, desired.as_slice()].concat()
            );
            assert_eq!(
                &edit.target_bytes[edit.managed_span],
                [expected_separator, desired.as_slice()].concat()
            );
        }
    }

    #[test]
    fn replaces_only_the_existing_block_and_preserves_arbitrary_exterior_bytes() {
        let old = block(b"old body\n");
        let desired = block(KNOWN_BODY);
        let current = [b"prefix\0bytes\n".as_slice(), &old, b"suffix\r\nbytes"].concat();
        let parsed = inspect_agents(Some(current)).unwrap();

        let edit = plan_agents_edit(parsed, KNOWN_BODY).unwrap();

        assert_eq!(
            edit.target_bytes,
            [b"prefix\0bytes\n".as_slice(), &desired, b"suffix\r\nbytes"].concat()
        );
        assert!(edit.inserted_separator.is_empty());
        assert_eq!(&edit.target_bytes[edit.managed_span], desired);

        let exact = inspect_agents(Some(edit.target_bytes.clone())).unwrap();
        assert_eq!(
            plan_agents_edit(exact, KNOWN_BODY).unwrap().target_bytes,
            edit.target_bytes
        );
    }

    #[test]
    fn admits_exactly_the_maximum_generated_block_size() {
        let overhead = block(b"\n").len() - 1;
        let mut exact_body = vec![b'x'; MAX_MANAGED_BLOCK_BYTES - overhead];
        *exact_body.last_mut().unwrap() = b'\n';
        let exact = plan_agents_edit(AgentsDocumentState::Absent, &exact_body).unwrap();
        assert_eq!(exact.target_bytes.len(), MAX_MANAGED_BLOCK_BYTES);

        let mut oversized_body = exact_body;
        oversized_body.insert(0, b'x');
        assert_eq!(
            plan_agents_edit(AgentsDocumentState::Absent, &oversized_body).unwrap_err(),
            Reason::ManagedFileConflict
        );
    }
}
