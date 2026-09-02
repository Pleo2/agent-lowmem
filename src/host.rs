use crate::result::Reason;
use serde::Serialize;
use sysctl::{Ctl, Sysctl};

const MACOS_VERSION_KEY: &str = "kern.osproductversion";
const HARDWARE_MODEL_KEY: &str = "hw.model";
const CPU_BRAND_KEY: &str = "machdep.cpu.brand_string";
const PHYSICAL_MEMORY_KEY: &str = "hw.memsize";
const PAGE_SIZE_KEY: &str = "hw.pagesize";

const REFERENCE_MACOS_MAJOR: u64 = 26;
const REFERENCE_HARDWARE_MODEL: &str = "Mac14,15";
const REFERENCE_CPU_BRAND: &str = "Apple M2";
const REFERENCE_PHYSICAL_MEMORY_BYTES: u64 = 8_589_934_592;
const REFERENCE_PAGE_SIZE_BYTES: u64 = 16_384;

pub trait HostSource {
    fn operating_system(&self) -> &str;
    fn architecture(&self) -> &str;
    fn read(&self, key: &'static str) -> Result<String, HostReadError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NativeHostSource;

impl HostSource for NativeHostSource {
    fn operating_system(&self) -> &str {
        std::env::consts::OS
    }

    fn architecture(&self) -> &str {
        std::env::consts::ARCH
    }

    fn read(&self, key: &'static str) -> Result<String, HostReadError> {
        let control = Ctl::new(key).map_err(|source| HostReadError::Sysctl(source.to_string()))?;
        control
            .value_string()
            .map(|value| value.trim().to_owned())
            .map_err(|source| HostReadError::Sysctl(source.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostReadError {
    Sysctl(String),
    Missing(&'static str),
    InvalidNumber(&'static str),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileField {
    OperatingSystem,
    Architecture,
    MacosMajor,
    HardwareModel,
    CpuBrand,
    PhysicalMemoryBytes,
    PageSizeBytes,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostReport {
    pub operating_system: String,
    pub architecture: String,
    pub macos_version: Option<String>,
    pub hardware_model: Option<String>,
    pub cpu_brand: Option<String>,
    pub physical_memory_bytes: Option<u64>,
    pub page_size_bytes: Option<u64>,
    pub runtime_supported: bool,
    pub performance_validated: bool,
    pub mismatched_profile_fields: Vec<ProfileField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<Reason>,
}

pub fn inspect_host(source: &impl HostSource) -> HostReport {
    let operating_system = normalize_operating_system(source.operating_system());
    let architecture = normalize_architecture(source.architecture());
    let macos_version = read_trimmed(source, MACOS_VERSION_KEY);
    let hardware_model = read_trimmed(source, HARDWARE_MODEL_KEY);
    let cpu_brand = read_trimmed(source, CPU_BRAND_KEY);
    let physical_memory = read_trimmed(source, PHYSICAL_MEMORY_KEY);
    let page_size = read_trimmed(source, PAGE_SIZE_KEY);

    let macos_major = macos_version
        .as_deref()
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse::<u64>().ok());
    let physical_memory_bytes = physical_memory
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok());
    let page_size_bytes = page_size
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok());

    let runtime_supported = operating_system == "darwin"
        && architecture == "arm64"
        && macos_major.is_some_and(|major| major >= 13)
        && macos_version.is_some()
        && hardware_model.is_some()
        && cpu_brand.is_some()
        && physical_memory_bytes.is_some()
        && page_size_bytes.is_some();

    let mut mismatched_profile_fields = Vec::new();
    push_mismatch(
        &mut mismatched_profile_fields,
        operating_system == "darwin",
        ProfileField::OperatingSystem,
    );
    push_mismatch(
        &mut mismatched_profile_fields,
        architecture == "arm64",
        ProfileField::Architecture,
    );
    push_mismatch(
        &mut mismatched_profile_fields,
        macos_major == Some(REFERENCE_MACOS_MAJOR),
        ProfileField::MacosMajor,
    );
    push_mismatch(
        &mut mismatched_profile_fields,
        hardware_model.as_deref() == Some(REFERENCE_HARDWARE_MODEL),
        ProfileField::HardwareModel,
    );
    push_mismatch(
        &mut mismatched_profile_fields,
        cpu_brand.as_deref() == Some(REFERENCE_CPU_BRAND),
        ProfileField::CpuBrand,
    );
    push_mismatch(
        &mut mismatched_profile_fields,
        physical_memory_bytes == Some(REFERENCE_PHYSICAL_MEMORY_BYTES),
        ProfileField::PhysicalMemoryBytes,
    );
    push_mismatch(
        &mut mismatched_profile_fields,
        page_size_bytes == Some(REFERENCE_PAGE_SIZE_BYTES),
        ProfileField::PageSizeBytes,
    );

    HostReport {
        operating_system,
        architecture,
        macos_version,
        hardware_model,
        cpu_brand,
        physical_memory_bytes,
        page_size_bytes,
        runtime_supported,
        performance_validated: runtime_supported && mismatched_profile_fields.is_empty(),
        mismatched_profile_fields,
        failure_reason: (!runtime_supported).then_some(Reason::HostUnsupported),
    }
}

fn normalize_operating_system(value: &str) -> String {
    match value {
        "macos" => "darwin".to_owned(),
        other => other.trim().to_ascii_lowercase(),
    }
}

fn normalize_architecture(value: &str) -> String {
    match value {
        "aarch64" => "arm64".to_owned(),
        other => other.trim().to_ascii_lowercase(),
    }
}

fn read_trimmed(source: &impl HostSource, key: &'static str) -> Option<String> {
    source.read(key).ok().map(|value| value.trim().to_owned())
}

fn push_mismatch(fields: &mut Vec<ProfileField>, matches: bool, field: ProfileField) {
    if !matches {
        fields.push(field);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::NativeHostSource;
    use super::{HostReadError, HostSource, ProfileField, inspect_host};
    use crate::result::Reason;
    use std::collections::BTreeMap;

    struct FakeHostSource {
        operating_system: &'static str,
        architecture: &'static str,
        values: BTreeMap<&'static str, &'static str>,
    }

    impl FakeHostSource {
        fn reference() -> Self {
            Self {
                operating_system: "macos",
                architecture: "aarch64",
                values: BTreeMap::from([
                    ("kern.osproductversion", "26.6.2\n"),
                    ("hw.model", "Mac14,15"),
                    ("machdep.cpu.brand_string", "Apple M2 "),
                    ("hw.memsize", "8589934592"),
                    ("hw.pagesize", "16384"),
                ]),
            }
        }
    }

    impl HostSource for FakeHostSource {
        fn operating_system(&self) -> &str {
            self.operating_system
        }

        fn architecture(&self) -> &str {
            self.architecture
        }

        fn read(&self, key: &'static str) -> Result<String, HostReadError> {
            self.values
                .get(key)
                .map(|value| value.trim().to_owned())
                .ok_or(HostReadError::Missing(key))
        }
    }

    #[test]
    fn matches_only_the_exact_reference_profile() {
        let source = FakeHostSource::reference();
        let report = inspect_host(&source);
        assert!(report.runtime_supported);
        assert!(report.performance_validated);
        assert!(report.mismatched_profile_fields.is_empty());
    }

    #[test]
    fn supports_a_capable_non_reference_mac_without_transferring_validation() {
        let mut source = FakeHostSource::reference();
        source.values.insert("hw.model", "Mac15,12");
        let report = inspect_host(&source);
        assert!(report.runtime_supported);
        assert!(!report.performance_validated);
        assert_eq!(
            report.mismatched_profile_fields,
            vec![ProfileField::HardwareModel]
        );
    }

    #[test]
    fn rejects_a_missing_mandatory_native_read() {
        let mut source = FakeHostSource::reference();
        source.values.remove("hw.pagesize");
        let report = inspect_host(&source);
        assert!(!report.runtime_supported);
        assert_eq!(report.failure_reason, Some(Reason::HostUnsupported));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_the_reference_mac_through_safe_native_sysctls() {
        let report = inspect_host(&NativeHostSource);

        assert_eq!(report.hardware_model.as_deref(), Some("Mac14,15"));
        assert_eq!(report.cpu_brand.as_deref(), Some("Apple M2"));
        assert_eq!(report.physical_memory_bytes, Some(8_589_934_592));
        assert_eq!(report.page_size_bytes, Some(16_384));

        if report
            .macos_version
            .as_deref()
            .and_then(|version| version.split('.').next())
            .and_then(|major| major.parse::<u64>().ok())
            == Some(26)
        {
            assert!(report.performance_validated);
        }
    }
}
