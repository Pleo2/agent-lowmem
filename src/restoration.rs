// The planner and transaction engine consume these primitives beginning in Task 8.
#![allow(dead_code)]

use crate::result::Reason;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str};

pub(crate) const MAX_MANIFEST_BYTES: usize = 262_144;
const MAX_CONFIGURATION_BYTES: usize = 262_144;
const MAX_MANAGED_BLOCK_BYTES: usize = 65_536;
const MAX_DOCUMENT_BYTES: u32 = 1_048_576;
const SCHEMA_VERSION: u8 = 1;
const FORMAT_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum JournalState {
    Prepared,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Ownership {
    Managed,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DestinationAction {
    Create,
    Replace,
    Remove,
    Unchanged,
    Preserve,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OwnedBytes {
    #[serde(with = "utf8_bytes")]
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
}

impl OwnedBytes {
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, Reason> {
        str::from_utf8(&bytes).map_err(|_| Reason::InternalError)?;
        let sha256 = hex_digest(&Sha256::digest(&bytes).into());
        Ok(Self { bytes, sha256 })
    }

    fn is_valid(&self, limit: usize) -> bool {
        let actual: [u8; 32] = Sha256::digest(&self.bytes).into();
        !self.bytes.is_empty()
            && self.bytes.len() <= limit
            && str::from_utf8(&self.bytes).is_ok()
            && parse_digest(&self.sha256).is_some_and(|expected| expected == actual)
    }
}

impl fmt::Debug for OwnedBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnedBytes")
            .field("byte_length", &self.bytes.len())
            .field("sha256", &self.sha256)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "owned", rename_all = "kebab-case")]
pub(crate) enum PriorManagedState {
    Absent,
    Bytes(OwnedBytes),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManagedSpan {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigurationRestoration {
    pub(crate) ownership: Ownership,
    pub(crate) action: DestinationAction,
    pub(crate) immediate_before: Option<PriorManagedState>,
    pub(crate) target: Option<OwnedBytes>,
    pub(crate) stable_baseline: Option<PriorManagedState>,
    pub(crate) before_mode: Option<u16>,
    pub(crate) target_mode: Option<u16>,
    pub(crate) external_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AgentsRestoration {
    pub(crate) ownership: Ownership,
    pub(crate) action: DestinationAction,
    pub(crate) document_was_absent: bool,
    pub(crate) immediate_before: PriorManagedState,
    pub(crate) target: Option<OwnedBytes>,
    pub(crate) stable_baseline: PriorManagedState,
    pub(crate) before_mode: Option<u16>,
    pub(crate) target_mode: Option<u16>,
    pub(crate) managed_span: ManagedSpan,
    #[serde(with = "utf8_bytes")]
    pub(crate) inserted_separator: Vec<u8>,
    pub(crate) prefix_sha256: String,
    pub(crate) suffix_sha256: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RestorationManifest {
    pub(crate) schema_version: u8,
    pub(crate) format_version: u8,
    pub(crate) state: JournalState,
    pub(crate) repository_sha256: String,
    pub(crate) transaction_sha256: String,
    pub(crate) configuration: ConfigurationRestoration,
    pub(crate) agents_policy: AgentsRestoration,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) previous_applied: Option<Box<RestorationManifest>>,
}

impl fmt::Debug for RestorationManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RestorationManifest")
            .field("schema_version", &self.schema_version)
            .field("format_version", &self.format_version)
            .field("state", &self.state)
            .field("repository_sha256", &self.repository_sha256)
            .field("transaction_sha256", &self.transaction_sha256)
            .field("configuration", &self.configuration)
            .field("agents_policy", &self.agents_policy)
            .field("has_previous_applied", &self.previous_applied.is_some())
            .finish()
    }
}

impl RestorationManifest {
    pub(crate) fn new(
        state: JournalState,
        repository_sha256: String,
        configuration: ConfigurationRestoration,
        agents_policy: AgentsRestoration,
        previous_applied: Option<Box<RestorationManifest>>,
    ) -> Result<Self, Reason> {
        let mut manifest = Self {
            schema_version: SCHEMA_VERSION,
            format_version: FORMAT_VERSION,
            state,
            repository_sha256,
            transaction_sha256: "0".repeat(64),
            configuration,
            agents_policy,
            previous_applied,
        };
        manifest.validate_without_transaction()?;
        manifest.transaction_sha256 = hex_digest(&transaction_digest(&manifest)?);
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), Reason> {
        self.validate_without_transaction()?;
        let declared = parse_digest(&self.transaction_sha256).ok_or(Reason::InternalError)?;
        if declared != transaction_digest(self)? {
            return Err(Reason::InternalError);
        }
        Ok(())
    }

    fn validate_without_transaction(&self) -> Result<(), Reason> {
        if self.schema_version != SCHEMA_VERSION
            || self.format_version != FORMAT_VERSION
            || parse_digest(&self.repository_sha256).is_none()
            || !valid_configuration(&self.configuration)
            || !valid_agents(&self.agents_policy)
        {
            return Err(Reason::InternalError);
        }
        if let Some(previous) = &self.previous_applied {
            if !matches!(previous.state, JournalState::Applied)
                || previous.previous_applied.is_some()
                || previous.repository_sha256 != self.repository_sha256
                || previous.validate().is_err()
            {
                return Err(Reason::InternalError);
            }
        }
        Ok(())
    }
}

pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<RestorationManifest, Reason> {
    if bytes.len() > MAX_MANIFEST_BYTES || str::from_utf8(bytes).is_err() {
        return Err(Reason::ManagedFileConflict);
    }
    let manifest: RestorationManifest =
        serde_json::from_slice(bytes).map_err(|_| Reason::ManagedFileConflict)?;
    manifest
        .validate()
        .map_err(|_| Reason::ManagedFileConflict)?;
    if serialize_unchecked(&manifest)? != bytes {
        return Err(Reason::ManagedFileConflict);
    }
    Ok(manifest)
}

pub(crate) fn serialize_manifest(manifest: &RestorationManifest) -> Result<Vec<u8>, Reason> {
    manifest.validate()?;
    let bytes = serialize_unchecked(manifest)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(Reason::InternalError);
    }
    Ok(bytes)
}

pub(crate) fn transaction_digest(manifest: &RestorationManifest) -> Result<[u8; 32], Reason> {
    manifest.validate_without_transaction()?;
    let value = serde_json::to_value(DigestInput {
        schema_version: manifest.schema_version,
        format_version: manifest.format_version,
        state: manifest.state,
        repository_sha256: &manifest.repository_sha256,
        configuration: &manifest.configuration,
        agents_policy: &manifest.agents_policy,
        previous_applied: manifest.previous_applied.as_deref(),
    })
    .map_err(|_| Reason::InternalError)?;
    let bytes = serde_json::to_vec(&value).map_err(|_| Reason::InternalError)?;
    Ok(Sha256::digest(bytes).into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestInput<'a> {
    schema_version: u8,
    format_version: u8,
    state: JournalState,
    repository_sha256: &'a str,
    configuration: &'a ConfigurationRestoration,
    agents_policy: &'a AgentsRestoration,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_applied: Option<&'a RestorationManifest>,
}

fn valid_configuration(configuration: &ConfigurationRestoration) -> bool {
    if !configuration.before_mode.is_none_or(valid_mode)
        || !configuration.target_mode.is_none_or(valid_mode)
    {
        return false;
    }
    match configuration.ownership {
        Ownership::External => {
            matches!(configuration.action, DestinationAction::Preserve)
                && configuration.immediate_before.is_none()
                && configuration.target.is_none()
                && configuration.stable_baseline.is_none()
                && configuration.before_mode.is_some()
                && configuration.target_mode.is_none()
                && configuration
                    .external_sha256
                    .as_deref()
                    .is_some_and(|digest| parse_digest(digest).is_some())
        }
        Ownership::Managed => {
            configuration.external_sha256.is_none()
                && configuration
                    .immediate_before
                    .as_ref()
                    .is_some_and(|state| valid_prior(state, MAX_CONFIGURATION_BYTES))
                && configuration
                    .stable_baseline
                    .as_ref()
                    .is_some_and(|state| valid_prior(state, MAX_CONFIGURATION_BYTES))
                && configuration
                    .target
                    .as_ref()
                    .is_none_or(|target| target.is_valid(MAX_CONFIGURATION_BYTES))
                && valid_managed_action(
                    configuration.action,
                    configuration.immediate_before.as_ref().expect("checked"),
                    configuration.target.as_ref(),
                    configuration.before_mode,
                    configuration.target_mode,
                )
        }
    }
}

fn valid_agents(agents: &AgentsRestoration) -> bool {
    let span_length = agents
        .managed_span
        .end
        .checked_sub(agents.managed_span.start);
    if !matches!(agents.ownership, Ownership::Managed)
        || matches!(agents.action, DestinationAction::Preserve)
        || agents.managed_span.end > MAX_DOCUMENT_BYTES
        || span_length.is_none_or(|length| length == 0)
        || !agents.before_mode.is_none_or(valid_mode)
        || !agents.target_mode.is_none_or(valid_mode)
        || !valid_prior(&agents.immediate_before, MAX_MANAGED_BLOCK_BYTES)
        || !valid_prior(&agents.stable_baseline, MAX_MANAGED_BLOCK_BYTES)
        || !agents
            .target
            .as_ref()
            .is_none_or(|target| target.is_valid(MAX_MANAGED_BLOCK_BYTES))
        || !matches!(agents.inserted_separator.as_slice(), b"" | b"\n" | b"\n\n")
        || parse_digest(&agents.prefix_sha256).is_none()
        || parse_digest(&agents.suffix_sha256).is_none()
    {
        return false;
    }
    let expected_length = match agents.action {
        DestinationAction::Remove => prior_bytes(&agents.immediate_before).map(Vec::len),
        _ => agents.target.as_ref().map(|target| target.bytes.len()),
    };
    expected_length.is_some_and(|length| span_length == u32::try_from(length).ok())
        && valid_managed_action(
            agents.action,
            &agents.immediate_before,
            agents.target.as_ref(),
            agents.before_mode,
            agents.target_mode,
        )
        && (!agents.document_was_absent
            || (matches!(agents.action, DestinationAction::Create)
                && matches!(agents.immediate_before, PriorManagedState::Absent)
                && agents.before_mode.is_none()
                && agents.managed_span.start == 0
                && agents.inserted_separator.is_empty()))
}

fn valid_managed_action(
    action: DestinationAction,
    immediate: &PriorManagedState,
    target: Option<&OwnedBytes>,
    before_mode: Option<u16>,
    target_mode: Option<u16>,
) -> bool {
    match action {
        DestinationAction::Create => {
            matches!(immediate, PriorManagedState::Absent)
                && target.is_some()
                && before_mode.is_none()
                && target_mode == Some(0o600)
        }
        DestinationAction::Replace => {
            matches!(immediate, PriorManagedState::Bytes(_))
                && target.is_some()
                && before_mode.is_some()
                && before_mode == target_mode
        }
        DestinationAction::Remove => {
            matches!(immediate, PriorManagedState::Bytes(_))
                && target.is_none()
                && before_mode.is_some()
                && target_mode.is_none()
        }
        DestinationAction::Unchanged => {
            prior_bytes(immediate)
                .is_some_and(|before| target.is_some_and(|target| before == &target.bytes))
                && before_mode.is_some()
                && before_mode == target_mode
        }
        DestinationAction::Preserve => false,
    }
}

fn valid_prior(state: &PriorManagedState, limit: usize) -> bool {
    match state {
        PriorManagedState::Absent => true,
        PriorManagedState::Bytes(owned) => owned.is_valid(limit),
    }
}

fn prior_bytes(state: &PriorManagedState) -> Option<&Vec<u8>> {
    match state {
        PriorManagedState::Absent => None,
        PriorManagedState::Bytes(owned) => Some(&owned.bytes),
    }
}

const fn valid_mode(mode: u16) -> bool {
    mode <= 0o666 && mode & 0o111 == 0
}

fn serialize_unchecked(manifest: &RestorationManifest) -> Result<Vec<u8>, Reason> {
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(|_| Reason::InternalError)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn parse_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        digest[index] = (high << 4) | low;
    }
    Some(digest)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

mod utf8_bytes {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
    use std::str;

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(str::from_utf8(bytes).map_err(serde::ser::Error::custom)?)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .map(String::into_bytes)
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentsRestoration, ConfigurationRestoration, DestinationAction, JournalState,
        MAX_MANIFEST_BYTES, ManagedSpan, OwnedBytes, Ownership, PriorManagedState,
        RestorationManifest, parse_manifest, serialize_manifest, transaction_digest,
    };
    use crate::result::Reason;
    use sha2::{Digest, Sha256};

    const REPOSITORY_SHA256: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn round_trips_prepared_and_applied_manifests_canonically() {
        for state in [JournalState::Prepared, JournalState::Applied] {
            let manifest = sample(state, None);
            let bytes = serialize_manifest(&manifest).unwrap();
            assert!(bytes.ends_with(b"\n"));
            assert!(!bytes.ends_with(b"\n\n"));
            assert_eq!(parse_manifest(&bytes).unwrap(), manifest);
            assert_eq!(
                transaction_digest(&manifest).unwrap(),
                digest_input(&manifest)
            );
        }

        let with_previous = sample(
            JournalState::Prepared,
            Some(Box::new(sample(JournalState::Applied, None))),
        );
        let bytes = serialize_manifest(&with_previous).unwrap();
        assert_eq!(parse_manifest(&bytes).unwrap(), with_previous);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("\n  \"schemaVersion\": 1,")
        );
    }

    #[test]
    fn rejects_bad_digests_spans_modes_owners_sizes_depth_and_noncanonical_json() {
        let valid = sample(JournalState::Prepared, None);
        let canonical = serialize_manifest(&valid).unwrap();
        let mut cases = Vec::new();

        let mut bad_repository = valid.clone();
        bad_repository.repository_sha256 = "ABC".into();
        cases.push(bad_repository);

        let mut bad_span = valid.clone();
        bad_span.agents_policy.managed_span.end = 0;
        cases.push(bad_span);

        let mut bad_mode = valid.clone();
        bad_mode.configuration.target_mode = Some(0o100);
        cases.push(bad_mode);

        let mut bad_owner = valid.clone();
        bad_owner.configuration.ownership = Ownership::External;
        cases.push(bad_owner);

        let mut oversized = valid.clone();
        oversized.agents_policy.target = Some(OwnedBytes::new(vec![b'x'; 65_537]).unwrap());
        oversized.agents_policy.managed_span.end = 65_537;
        cases.push(oversized);

        let prior = sample(JournalState::Applied, None);
        let mut recursive = sample(JournalState::Applied, None);
        recursive.previous_applied = Some(Box::new(prior));
        let mut too_deep = sample(JournalState::Prepared, None);
        too_deep.previous_applied = Some(Box::new(recursive));
        cases.push(too_deep);

        for manifest in cases {
            assert_eq!(serialize_manifest(&manifest), Err(Reason::InternalError));
        }

        let mut wrong_digest: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
        wrong_digest["transactionSha256"] = serde_json::Value::String(REPOSITORY_SHA256.into());
        let wrong_digest = canonical_json(&wrong_digest);
        assert_eq!(
            parse_manifest(&wrong_digest),
            Err(Reason::ManagedFileConflict)
        );

        let compact =
            serde_json::to_vec(&serde_json::from_slice::<serde_json::Value>(&canonical).unwrap())
                .unwrap();
        assert_eq!(parse_manifest(&compact), Err(Reason::ManagedFileConflict));

        let mut unknown: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
        unknown["absoluteRoot"] = serde_json::Value::String("/private/root".into());
        assert_eq!(
            parse_manifest(&canonical_json(&unknown)),
            Err(Reason::ManagedFileConflict)
        );

        assert_eq!(
            parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1]),
            Err(Reason::ManagedFileConflict)
        );
    }

    #[test]
    fn never_serializes_external_or_surrounding_bytes_and_debug_is_redacted() {
        let mut manifest = sample(JournalState::Applied, None);
        manifest.configuration = ConfigurationRestoration {
            ownership: Ownership::External,
            action: DestinationAction::Preserve,
            immediate_before: None,
            target: None,
            stable_baseline: None,
            before_mode: Some(0o640),
            target_mode: None,
            external_sha256: Some(REPOSITORY_SHA256.into()),
        };
        manifest = RestorationManifest::new(
            manifest.state,
            manifest.repository_sha256,
            manifest.configuration,
            manifest.agents_policy,
            manifest.previous_applied,
        )
        .unwrap();
        let serialized = String::from_utf8(serialize_manifest(&manifest).unwrap()).unwrap();
        let debug = format!("{manifest:?}");
        for forbidden in [
            "/Users/private/repository",
            "piolinos",
            "TOKEN=secret",
            "manual configuration bytes",
            "private AGENTS prefix",
            "private AGENTS suffix",
        ] {
            assert!(!serialized.contains(forbidden));
            assert!(!debug.contains(forbidden));
        }
        assert!(!debug.contains("managed config"));
        assert!(!debug.contains("managed block"));

        let mut attempted_leak = manifest;
        attempted_leak.configuration.target =
            Some(OwnedBytes::new(b"manual configuration bytes".to_vec()).unwrap());
        assert_eq!(
            serialize_manifest(&attempted_leak),
            Err(Reason::InternalError)
        );

        let mut attempted_exterior_leak = sample(JournalState::Applied, None);
        attempted_exterior_leak.agents_policy.inserted_separator =
            b"private AGENTS prefix".to_vec();
        assert_eq!(
            serialize_manifest(&attempted_exterior_leak),
            Err(Reason::InternalError)
        );
    }

    fn sample(
        state: JournalState,
        previous_applied: Option<Box<RestorationManifest>>,
    ) -> RestorationManifest {
        RestorationManifest::new(
            state,
            REPOSITORY_SHA256.into(),
            ConfigurationRestoration {
                ownership: Ownership::Managed,
                action: DestinationAction::Create,
                immediate_before: Some(PriorManagedState::Absent),
                target: Some(OwnedBytes::new(b"managed config\n".to_vec()).unwrap()),
                stable_baseline: Some(PriorManagedState::Absent),
                before_mode: None,
                target_mode: Some(0o600),
                external_sha256: None,
            },
            AgentsRestoration {
                ownership: Ownership::Managed,
                action: DestinationAction::Create,
                document_was_absent: true,
                immediate_before: PriorManagedState::Absent,
                target: Some(OwnedBytes::new(b"managed block\n".to_vec()).unwrap()),
                stable_baseline: PriorManagedState::Absent,
                before_mode: None,
                target_mode: Some(0o600),
                managed_span: ManagedSpan { start: 0, end: 14 },
                inserted_separator: Vec::new(),
                prefix_sha256: EMPTY_SHA256.into(),
                suffix_sha256: EMPTY_SHA256.into(),
            },
            previous_applied,
        )
        .unwrap()
    }

    fn digest_input(manifest: &RestorationManifest) -> [u8; 32] {
        let mut value = serde_json::to_value(manifest).unwrap();
        value.as_object_mut().unwrap().remove("transactionSha256");
        Sha256::digest(serde_json::to_vec(&value).unwrap()).into()
    }

    fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(value).unwrap();
        bytes.push(b'\n');
        bytes
    }
}
