use crate::{managed_files::ManagedFilesReport, result::ExitResult};

const WORDMARK: &str = "agent_lowmem";
const RESET: &str = "\u{1b}[0m";
const GRADIENT_STOPS: [(f64, [u8; 3]); 4] = [
    (0.0, [0xc9, 0xb6, 0xff]),
    (0.38, [0x8b, 0x83, 0xff]),
    (0.70, [0x4f, 0x6c, 0xff]),
    (1.0, [0x50, 0xd8, 0xff]),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCapabilities {
    pub is_terminal: bool,
    pub no_color: bool,
    pub term: Option<String>,
    pub colorterm: Option<String>,
}

impl TerminalCapabilities {
    pub fn from_environment(is_terminal: bool) -> Self {
        Self {
            is_terminal,
            no_color: std::env::var_os("NO_COLOR").is_some(),
            term: std::env::var("TERM").ok(),
            colorterm: std::env::var("COLORTERM").ok(),
        }
    }

    fn truecolor_enabled(&self) -> bool {
        self.is_terminal
            && !self.no_color
            && self.term.as_deref() != Some("dumb")
            && self.colorterm.as_deref().is_some_and(|value| {
                value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
            })
    }
}

pub fn render_wordmark(capabilities: &TerminalCapabilities) -> String {
    if !capabilities.truecolor_enabled() {
        return WORDMARK.to_owned();
    }
    let final_index = WORDMARK.chars().count() - 1;
    WORDMARK
        .chars()
        .enumerate()
        .fold(String::new(), |mut output, (index, character)| {
            let position = index as f64 / final_index as f64;
            let color = interpolate_color(position);
            use std::fmt::Write as _;
            write!(
                output,
                "\u{1b}[38;2;{};{};{}m{character}{RESET}",
                color[0], color[1], color[2]
            )
            .expect("writing to a String cannot fail");
            output
        })
}

pub fn stable_result_line(result: ExitResult) -> String {
    format!(
        "agent-lowmem: result origin={} code={} reason={}",
        result.origin.as_str(),
        result.code,
        result.reason.as_str()
    )
}

pub fn stable_managed_files_line(report: &ManagedFilesReport) -> String {
    format!(
        "agent-lowmem: managed-files command={} outcome={} code={} reason={}",
        report.command.as_str(),
        report.outcome.as_str(),
        report.result.code,
        report.result.reason.as_str()
    )
}

fn interpolate_color(position: f64) -> [u8; 3] {
    let upper = GRADIENT_STOPS
        .iter()
        .position(|(stop, _)| position <= *stop)
        .unwrap_or(GRADIENT_STOPS.len() - 1);
    let lower = upper.saturating_sub(1);
    let (lower_position, lower_color) = GRADIENT_STOPS[lower];
    let (upper_position, upper_color) = GRADIENT_STOPS[upper];
    let progress = if upper_position == lower_position {
        0.0
    } else {
        (position - lower_position) / (upper_position - lower_position)
    };
    std::array::from_fn(|channel| {
        (f64::from(lower_color[channel])
            + (f64::from(upper_color[channel]) - f64::from(lower_color[channel])) * progress)
            .round() as u8
    })
}

#[cfg(test)]
mod tests {
    use super::{TerminalCapabilities, render_wordmark, stable_managed_files_line};
    use crate::{
        managed_files::{
            ManagedCommand, ManagedFilesReport, ManagedOutcome, ManagedResult, ManifestState,
        },
        result::Reason,
    };

    #[test]
    fn managed_files_line_is_stable_plain_text() {
        let report = ManagedFilesReport::new(
            ManagedCommand::Init,
            false,
            ManagedOutcome::Applied,
            ManagedResult::new(0, Reason::Completed).unwrap(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ManifestState::Applied,
        )
        .unwrap();

        assert_eq!(
            stable_managed_files_line(&report),
            "agent-lowmem: managed-files command=init outcome=applied code=0 reason=completed"
        );
        assert!(!stable_managed_files_line(&report).contains('\u{1b}'));
    }

    #[test]
    fn renders_the_exact_brand_gradient_with_an_immediate_reset_per_character() {
        let capabilities = TerminalCapabilities {
            is_terminal: true,
            no_color: false,
            term: Some("xterm-256color".to_owned()),
            colorterm: Some("truecolor".to_owned()),
        };

        assert_eq!(
            render_wordmark(&capabilities),
            concat!(
                "\u{1b}[38;2;201;182;255ma\u{1b}[0m",
                "\u{1b}[38;2;186;170;255mg\u{1b}[0m",
                "\u{1b}[38;2;171;158;255me\u{1b}[0m",
                "\u{1b}[38;2;157;145;255mn\u{1b}[0m",
                "\u{1b}[38;2;142;133;255mt\u{1b}[0m",
                "\u{1b}[38;2;125;126;255m_\u{1b}[0m",
                "\u{1b}[38;2;108;119;255ml\u{1b}[0m",
                "\u{1b}[38;2;91;113;255mo\u{1b}[0m",
                "\u{1b}[38;2;79;118;255mw\u{1b}[0m",
                "\u{1b}[38;2;79;151;255mm\u{1b}[0m",
                "\u{1b}[38;2;80;183;255me\u{1b}[0m",
                "\u{1b}[38;2;80;216;255mm\u{1b}[0m"
            )
        );
    }

    #[test]
    fn color_gates_preserve_identical_plain_text() {
        for capabilities in [
            TerminalCapabilities {
                is_terminal: false,
                no_color: false,
                term: Some("xterm-256color".to_owned()),
                colorterm: Some("truecolor".to_owned()),
            },
            TerminalCapabilities {
                is_terminal: true,
                no_color: true,
                term: Some("xterm-256color".to_owned()),
                colorterm: Some("truecolor".to_owned()),
            },
            TerminalCapabilities {
                is_terminal: true,
                no_color: false,
                term: Some("dumb".to_owned()),
                colorterm: Some("truecolor".to_owned()),
            },
            TerminalCapabilities {
                is_terminal: true,
                no_color: false,
                term: Some("xterm-256color".to_owned()),
                colorterm: None,
            },
        ] {
            assert_eq!(render_wordmark(&capabilities), "agent_lowmem");
        }
    }
}
