//! Data models for static analysis reports and diagnostics.

/// Severity level of a lint finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LintSeverity {
    /// Information or notice
    Info,
    /// Warning about potential issue or non-fatal anomaly
    Warning,
    /// Error that crashes the game engine or aborts script execution
    Error,
}

impl LintSeverity {
    /// Returns the uppercase string name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

/// A single static checker finding.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LintFinding {
    /// Name of checker that produced this finding (e.g. `check_condlists`, `check_dialogs`)
    pub checker: String,
    /// Relative or absolute path of the file
    pub file: String,
    /// 1-based line number (0 if whole-file or synthetic)
    pub line: usize,
    /// Severity level
    pub severity: LintSeverity,
    /// Diagnostic description
    pub message: String,
}

impl LintFinding {
    /// Formats the finding in standard compiler format (`path:line: checker: message`).
    #[must_use]
    pub fn to_display_string(&self) -> String {
        if self.line > 0 {
            format!("{}:{}: {}: {}", self.file, self.line, self.checker, self.message)
        } else {
            format!("{}: {}: {}", self.file, self.checker, self.message)
        }
    }
}

/// Consolidated lint summary report.
#[derive(Debug, Clone, Default)]
pub struct LintReport {
    /// Total files inspected
    pub files_checked: usize,
    /// Elapsed execution time in milliseconds
    pub elapsed_ms: u64,
    /// List of all findings
    pub findings: Vec<LintFinding>,
}

impl LintReport {
    /// Returns true if any error findings were recorded.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == LintSeverity::Error)
    }

    /// Returns count of findings by severity.
    #[must_use]
    pub fn count_by_severity(&self, severity: LintSeverity) -> usize {
        self.findings.iter().filter(|f| f.severity == severity).count()
    }
}
