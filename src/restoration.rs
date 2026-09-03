// The planner and transaction engine consume these primitives beginning in Task 8.
#![allow(dead_code)]

use crate::{
    agents_policy::{AgentsDocumentState, MAX_AGENTS_BYTES, inspect_agents},
    atomic_file::{FilePrecondition, HeldDirectory, OptionalFile},
    result::Reason,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str};

pub(crate) const MAX_MANIFEST_BYTES: usize = 262_144;
pub(crate) const MAX_CONFIGURATION_BYTES: usize = 262_144;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoverySide {
    Before,
    Target,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RecoveryPlan {
    manifest: RestorationManifest,
    configuration: OptionalFile,
    configuration_side: RecoverySide,
    agents: OptionalFile,
    agents_side: RecoverySide,
    agents_span: Option<std::ops::Range<usize>>,
}

impl fmt::Debug for RecoveryPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecoveryPlan")
            .field("manifest", &self.manifest)
            .field("configuration_side", &self.configuration_side)
            .field("agents_side", &self.agents_side)
            .field("agents_span", &self.agents_span)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecoveryClassification {
    NotRequired,
    Recoverable(RecoveryPlan),
    Conflict,
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

pub(crate) fn classify_prepared(
    repository: &HeldDirectory,
    manifest: &RestorationManifest,
) -> Result<RecoveryClassification, Reason> {
    if manifest.state != JournalState::Prepared {
        return Ok(RecoveryClassification::NotRequired);
    }
    let configuration = repository.read_optional(".agent-lowmem.json", MAX_CONFIGURATION_BYTES)?;
    let Some(configuration_side) = classify_configuration(&configuration, &manifest.configuration)
    else {
        return Ok(RecoveryClassification::Conflict);
    };
    let agents = repository.read_optional("AGENTS.md", MAX_AGENTS_BYTES)?;
    let Some((agents_side, agents_span)) = classify_agents(&agents, &manifest.agents_policy) else {
        return Ok(RecoveryClassification::Conflict);
    };
    Ok(RecoveryClassification::Recoverable(RecoveryPlan {
        manifest: manifest.clone(),
        configuration,
        configuration_side,
        agents,
        agents_side,
        agents_span,
    }))
}

pub(crate) fn recover_prepared(
    repository: &HeldDirectory,
    metadata: &HeldDirectory,
    plan: &RecoveryPlan,
) -> Result<(), Reason> {
    let private = HeldDirectory::open_child(metadata, "agent-lowmem", Some(0o700))?;
    let journal = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    let prepared_bytes = serialize_manifest(&plan.manifest)?;
    if !matches!(
        &journal,
        OptionalFile::Regular {
            bytes,
            mode: 0o600,
            owner_uid,
            ..
        } if bytes == &prepared_bytes && *owner_uid == rustix::process::geteuid().as_raw()
    ) {
        return Err(Reason::ManagedFileConflict);
    }

    if plan.agents_side == RecoverySide::Target {
        recover_agents(repository, plan)?;
    }
    if plan.configuration_side == RecoverySide::Target {
        recover_configuration(repository, plan)?;
    }

    let current = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    if optional_bytes(&current) != Some(prepared_bytes.as_slice()) {
        return Err(Reason::ManagedFileConflict);
    }
    match plan.manifest.previous_applied.as_deref() {
        Some(previous) => private.replace_atomic(
            "restoration-v1.json",
            &FilePrecondition::from(&current),
            &serialize_manifest(previous)?,
            0o600,
        )?,
        None => private.remove_exact("restoration-v1.json", &FilePrecondition::from(&current))?,
    }
    Ok(())
}

fn classify_configuration(
    current: &OptionalFile,
    configuration: &ConfigurationRestoration,
) -> Option<RecoverySide> {
    if configuration.ownership == Ownership::External {
        return matches!(
            current,
            OptionalFile::Regular {
                sha256,
                mode,
                ..
            } if Some(*mode as u16) == configuration.before_mode
                && configuration.external_sha256.as_deref() == Some(hex_digest(sha256).as_str())
        )
        .then_some(RecoverySide::Before);
    }
    let immediate = configuration.immediate_before.as_ref()?;
    if optional_matches_prior(current, immediate, configuration.before_mode) {
        return Some(RecoverySide::Before);
    }
    optional_matches_target(
        current,
        configuration.target.as_ref(),
        configuration.target_mode,
    )
    .then_some(RecoverySide::Target)
}

fn classify_agents(
    current: &OptionalFile,
    agents: &AgentsRestoration,
) -> Option<(RecoverySide, Option<std::ops::Range<usize>>)> {
    if agents_before_matches(current, agents) {
        return Some((RecoverySide::Before, None));
    }
    let OptionalFile::Regular { bytes, mode, .. } = current else {
        return None;
    };
    if Some(*mode as u16) != agents.target_mode {
        return None;
    }
    let AgentsDocumentState::OneBlock(block) = inspect_agents(Some(bytes.clone())).ok()? else {
        return None;
    };
    let target = agents.target.as_ref()?;
    let separator_length = if matches!(agents.immediate_before, PriorManagedState::Absent) {
        agents.inserted_separator.len()
    } else {
        0
    };
    let owned_start = block.span.start.checked_sub(separator_length)?;
    let owned_span = owned_start..block.span.end;
    if bytes.get(owned_span.clone()) != Some(target.bytes.as_slice())
        || owned_span.start != agents.managed_span.start as usize
        || owned_span.end != agents.managed_span.end as usize
        || digest(&bytes[..owned_span.start]) != agents.prefix_sha256
        || digest(&bytes[owned_span.end..]) != agents.suffix_sha256
    {
        return None;
    }
    Some((RecoverySide::Target, Some(owned_span)))
}

fn agents_before_matches(current: &OptionalFile, agents: &AgentsRestoration) -> bool {
    match (&agents.immediate_before, current) {
        (PriorManagedState::Absent, OptionalFile::Absent) => agents.before_mode.is_none(),
        (PriorManagedState::Absent, OptionalFile::Regular { bytes, mode, .. }) => {
            Some(*mode as u16) == agents.before_mode
                && matches!(
                    inspect_agents(Some(bytes.clone())),
                    Ok(AgentsDocumentState::NoBlock { .. })
                )
                && bytes.len() == agents.managed_span.start as usize
                && digest(bytes) == agents.prefix_sha256
                && digest(&[]) == agents.suffix_sha256
        }
        (PriorManagedState::Bytes(immediate), OptionalFile::Regular { bytes, mode, .. }) => {
            if Some(*mode as u16) != agents.before_mode {
                return false;
            }
            let Ok(AgentsDocumentState::OneBlock(block)) = inspect_agents(Some(bytes.clone()))
            else {
                return false;
            };
            block.managed_bytes() == immediate.bytes
                && digest(&bytes[..block.span.start]) == agents.prefix_sha256
                && digest(&bytes[block.span.end..]) == agents.suffix_sha256
        }
        _ => false,
    }
}

fn optional_matches_prior(
    current: &OptionalFile,
    expected: &PriorManagedState,
    expected_mode: Option<u16>,
) -> bool {
    match (current, expected) {
        (OptionalFile::Absent, PriorManagedState::Absent) => expected_mode.is_none(),
        (OptionalFile::Regular { bytes, mode, .. }, PriorManagedState::Bytes(expected)) => {
            bytes == &expected.bytes && Some(*mode as u16) == expected_mode
        }
        _ => false,
    }
}

fn optional_matches_target(
    current: &OptionalFile,
    target: Option<&OwnedBytes>,
    target_mode: Option<u16>,
) -> bool {
    match (current, target) {
        (OptionalFile::Absent, None) => target_mode.is_none(),
        (OptionalFile::Regular { bytes, mode, .. }, Some(target)) => {
            bytes == &target.bytes && Some(*mode as u16) == target_mode
        }
        _ => false,
    }
}

fn recover_configuration(repository: &HeldDirectory, plan: &RecoveryPlan) -> Result<(), Reason> {
    let before = plan
        .manifest
        .configuration
        .immediate_before
        .as_ref()
        .ok_or(Reason::InternalError)?;
    restore_prior(
        repository,
        ".agent-lowmem.json",
        &plan.configuration,
        before,
        plan.manifest.configuration.before_mode,
    )
}

fn recover_agents(repository: &HeldDirectory, plan: &RecoveryPlan) -> Result<(), Reason> {
    let OptionalFile::Regular { bytes, .. } = &plan.agents else {
        return Err(Reason::InternalError);
    };
    let span = plan.agents_span.clone().ok_or(Reason::InternalError)?;
    let agents = &plan.manifest.agents_policy;
    let replacement = match &agents.immediate_before {
        PriorManagedState::Absent => &[][..],
        PriorManagedState::Bytes(before) => before.bytes.as_slice(),
    };
    let mut restored = Vec::with_capacity(bytes.len() - span.len() + replacement.len());
    restored.extend_from_slice(&bytes[..span.start]);
    restored.extend_from_slice(replacement);
    restored.extend_from_slice(&bytes[span.end..]);
    if restored.is_empty() && agents.document_was_absent {
        return repository.remove_exact("AGENTS.md", &FilePrecondition::from(&plan.agents));
    }
    let mode = agents.before_mode.ok_or(Reason::InternalError)? as u32;
    repository.replace_atomic(
        "AGENTS.md",
        &FilePrecondition::from(&plan.agents),
        &restored,
        mode,
    )
}

fn restore_prior(
    directory: &HeldDirectory,
    name: &str,
    current: &OptionalFile,
    prior: &PriorManagedState,
    mode: Option<u16>,
) -> Result<(), Reason> {
    match prior {
        PriorManagedState::Absent => directory.remove_exact(name, &FilePrecondition::from(current)),
        PriorManagedState::Bytes(before) => directory.replace_atomic(
            name,
            &FilePrecondition::from(current),
            &before.bytes,
            mode.ok_or(Reason::InternalError)? as u32,
        ),
    }
}

fn optional_bytes(file: &OptionalFile) -> Option<&[u8]> {
    match file {
        OptionalFile::Absent => None,
        OptionalFile::Regular { bytes, .. } => Some(bytes),
    }
}

fn digest(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes).into())
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
        && valid_agents_action(
            agents.action,
            &agents.immediate_before,
            agents.target.as_ref(),
            agents.before_mode,
            agents.target_mode,
        )
        && (!agents.document_was_absent
            || (matches!(agents.stable_baseline, PriorManagedState::Absent)
                && agents.inserted_separator.is_empty()))
}

fn valid_agents_action(
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
                && match before_mode {
                    None => target_mode == Some(0o600),
                    Some(mode) => target_mode == Some(mode),
                }
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
