//! Approval records: the accountable judgement an approver writes down.
//!
//! A record binds a person to a decision about one node at one point in its
//! history: `approver` judged `target` as the content `view_hash` names, and
//! said `approve` or `decline`. A decline is a veto regardless of quorum, and
//! it must say why — a veto nobody can answer is not accountability, so a
//! decline without a `comment` is not a record at all.
//!
//! The struct's fields are private and every way in validates — the
//! constructors for a record being written, the deserializer for one being
//! read back — so a `Record` in hand is a well-formed v1 record, wherever it
//! came from. What "well-formed" means is fixed by `spec.md` → *Approval
//! records (v1)* and frozen by the golden vectors under
//! `tests/golden/record/v1/`: an incompatible change ships as a parallel v2,
//! never as an edit to what these fields mean.
//!
//! Two fields deliberately look past the proof of concept. `sig` is reserved
//! and must stay empty until signatures land; [`Record::canonical_bytes`] already defines
//! what will be signed — the sorted-key compact JSON of the record minus
//! `sig`, absent optional fields serialized as `null` — so introducing
//! signatures later changes no stored byte. `timestamp` is audit trail only:
//! evaluation never reads it, because whether an approval counts is decided by
//! content hashes and the declared phase order, not by wall clocks.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::hash::{InvalidDigest, ViewHash};
use crate::view::Mode;

/// The record layout version this build writes and reads.
const RECORD_VERSION: u64 = 1;

/// The judgement a record carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    Approve,
    /// A veto regardless of quorum; carries a mandatory `comment`.
    Decline,
}

/// The node a record judges, spelled `<kind>:<id>`.
///
/// The kind is written down rather than looked up so a record stays legible
/// on its own: renaming or re-moding a block cannot silently re-aim an old
/// approval, it just stops matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    kind: Mode,
    id: String,
}

impl Target {
    /// Builds a target from its parts, validating the id.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTarget`] when `id` is not an identifier
    /// (`[A-Za-z_][A-Za-z0-9_-]*`), optionally namespace-qualified with `::`.
    pub fn new(kind: Mode, id: impl Into<String>) -> Result<Self, InvalidTarget> {
        let id = id.into();
        if !is_reference(&id) {
            return Err(InvalidTarget(format!("{}:{id}", kind_name(kind))));
        }
        Ok(Self { kind, id })
    }

    /// The mode the record believes the target has.
    #[must_use]
    pub fn kind(&self) -> Mode {
        self.kind
    }

    /// The block id, namespace-qualified where the block is.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Displays as `<kind>:<id>`, the exact spelling the record stores.
impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", kind_name(self.kind), self.id)
    }
}

impl FromStr for Target {
    type Err = InvalidTarget;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidTarget(text.to_owned());
        let (kind, id) = text.split_once(':').ok_or_else(invalid)?;
        let kind = parse_kind(kind).ok_or_else(invalid)?;
        Self::new(kind, id).map_err(|_| invalid())
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A target string that is not `<kind>:<id>` with a known kind and a valid id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected a `<kind>:<id>` target with kind objective|structure|bind|fiat, found `{0}`")]
pub struct InvalidTarget(pub String);

/// The spelling of a mode inside a target, matching the DSL keywords.
fn kind_name(kind: Mode) -> &'static str {
    kind.name()
}

fn parse_kind(text: &str) -> Option<Mode> {
    match text {
        "objective" => Some(Mode::Objective),
        "structure" => Some(Mode::Structure),
        "bind" => Some(Mode::Bind),
        "fiat" => Some(Mode::Fiat),
        _ => None,
    }
}

/// One identifier: `[A-Za-z_][A-Za-z0-9_-]*`.
///
/// Mirrors the lexer's identifier grammar; records and the DSL must agree on
/// what an id is, so a grammar change there must be reflected here.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// An identifier, optionally namespace-qualified with `::`.
fn is_reference(text: &str) -> bool {
    !text.is_empty() && text.split("::").all(is_identifier)
}

/// An RFC3339 UTC instant, audit trail only: eval never reads it.
///
/// Only the `YYYY-MM-DDThh:mm:ss(.digits)?Z` spelling is accepted — uppercase
/// `T` and `Z`, no numeric offset — so every stored timestamp reads the same
/// way and none smuggles in a local zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Timestamp(String);

impl Timestamp {
    /// The stored spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Timestamp {
    type Err = InvalidTimestamp;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if !is_rfc3339_utc(text) {
            return Err(InvalidTimestamp(text.to_owned()));
        }
        Ok(Self(text.to_owned()))
    }
}

/// A timestamp that is not an RFC3339 UTC instant in the stored spelling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected an RFC3339 UTC timestamp like `2026-09-10T08:30:00Z`, found `{0}`")]
pub struct InvalidTimestamp(pub String);

/// Where the digits of `YYYY-MM-DDThh:mm:ss` sit.
const DIGIT_POSITIONS: [usize; 14] = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];

/// Whether `text` is `YYYY-MM-DDThh:mm:ss(.digits)?Z` naming a real instant.
fn is_rfc3339_utc(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[bytes.len() - 1] != b'Z' {
        return false;
    }

    let separators_placed = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':';
    if !separators_placed || DIGIT_POSITIONS.iter().any(|&i| !bytes[i].is_ascii_digit()) {
        return false;
    }

    let fraction_valid = match &bytes[19..bytes.len() - 1] {
        [] => true,
        [b'.', digits @ ..] => !digits.is_empty() && digits.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    if !fraction_valid {
        return false;
    }

    let field = |start: usize, end: usize| -> u32 {
        text[start..end].parse().expect("checked ASCII digits")
    };
    let (year, month, day) = (field(0, 4), field(5, 7), field(8, 10));
    let (hour, minute, second) = (field(11, 13), field(14, 16), field(17, 19));
    (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        // RFC3339 admits a leap second.
        && second <= 60
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap_year(year) => 29,
        _ => 28,
    }
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// The fields every record carries whatever its label.
///
/// [`Record::approve`] and [`Record::decline`] take these plus the
/// label-specific comment, so the one place a decline differs from an approval
/// is spelled at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFields {
    pub target: Target,
    pub view_hash: ViewHash,
    pub approver: String,
    /// Names the key that will sign once signatures land.
    pub key_id: String,
    pub timestamp: Timestamp,
    /// The current phase at approval time.
    pub phase: String,
    /// An approver-side reservation: a phase this judgement is placed toward.
    pub until: Option<String>,
    /// The policy the approval was granted under, once policy lands.
    ///
    /// A `sha256:<hex>` digest spelling, validated but deliberately untyped:
    /// the policy layout has not landed, so the record commits only to the
    /// spelling, not to what it identifies.
    pub policy_hash: Option<String>,
}

/// One v1 approval record. See the module docs for what the type guarantees.
///
/// Field order is the storage order: a record serializes its fields exactly as
/// `spec.md` lists them. The canonical *signing* bytes sort keys instead — see
/// [`Record::canonical_bytes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Record {
    v: u64,
    target: Target,
    view_hash: ViewHash,
    label: Label,
    comment: Option<String>,
    approver: String,
    key_id: String,
    timestamp: Timestamp,
    phase: String,
    until: Option<String>,
    policy_hash: Option<String>,
    /// Reserved; empty until signatures land.
    sig: String,
}

impl Record {
    /// An approval of the content `fields.view_hash` names.
    ///
    /// # Errors
    ///
    /// Returns a [`RecordError`] when a field breaks a record invariant; see
    /// [`RecordError`] for the full list.
    pub fn approve(fields: RecordFields, comment: Option<String>) -> Result<Self, RecordError> {
        Self::new(Label::Approve, comment, fields)
    }

    /// A decline: a veto regardless of quorum, with the mandatory `comment`
    /// saying why.
    ///
    /// # Errors
    ///
    /// Returns a [`RecordError`] when a field breaks a record invariant; see
    /// [`RecordError`] for the full list.
    pub fn decline(fields: RecordFields, comment: String) -> Result<Self, RecordError> {
        Self::new(Label::Decline, Some(comment), fields)
    }

    fn new(
        label: Label,
        comment: Option<String>,
        fields: RecordFields,
    ) -> Result<Self, RecordError> {
        let record = Self {
            v: RECORD_VERSION,
            target: fields.target,
            view_hash: fields.view_hash,
            label,
            comment,
            approver: fields.approver,
            key_id: fields.key_id,
            timestamp: fields.timestamp,
            phase: fields.phase,
            until: fields.until,
            policy_hash: fields.policy_hash,
            sig: String::new(),
        };
        record.validate()?;
        Ok(record)
    }

    /// The one place every record invariant is spelled: both construction and
    /// deserialization end here.
    fn validate(&self) -> Result<(), RecordError> {
        if self.v != RECORD_VERSION {
            return Err(RecordError::UnsupportedVersion { found: self.v });
        }
        if !is_safe_file_stem(&self.approver) {
            return Err(RecordError::InvalidApprover(self.approver.clone()));
        }
        if self.key_id.trim().is_empty() {
            return Err(RecordError::BlankKeyId);
        }
        if self.phase.trim().is_empty() {
            return Err(RecordError::BlankPhase);
        }
        match &self.comment {
            Some(comment) if comment.trim().is_empty() => return Err(RecordError::BlankComment),
            None if self.label == Label::Decline => {
                return Err(RecordError::MissingDeclineComment);
            }
            _ => {}
        }
        if self
            .until
            .as_ref()
            .is_some_and(|until| until.trim().is_empty())
        {
            return Err(RecordError::BlankUntil);
        }
        // A comment is prose and may break lines; the single-valued fields
        // may not carry any control character at all.
        let single_valued = [
            ("key_id", Some(self.key_id.as_str())),
            ("phase", Some(self.phase.as_str())),
            ("until", self.until.as_deref()),
        ];
        for (field, value) in single_valued {
            if value.is_some_and(|value| has_forbidden_control(value, false)) {
                return Err(RecordError::ControlCharacters { field });
            }
        }
        if self
            .comment
            .as_deref()
            .is_some_and(|comment| has_forbidden_control(comment, true))
        {
            return Err(RecordError::ControlCharacters { field: "comment" });
        }
        if let Some(policy_hash) = &self.policy_hash {
            policy_hash.parse::<ViewHash>()?;
        }
        if !self.sig.is_empty() {
            return Err(RecordError::ReservedSignature);
        }
        Ok(())
    }

    /// The bytes a future signature will be taken over: the sorted-key compact
    /// JSON of the record minus `sig`, absent optional fields as `null`.
    ///
    /// Sorting is done here rather than left to the JSON library's map order,
    /// so a feature flag elsewhere in a build can never reorder what gets
    /// signed. The result is frozen by the golden vectors.
    ///
    /// # Panics
    ///
    /// Panics if the record fails to serialize to JSON, which a validated
    /// record cannot do.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let serde_json::Value::Object(fields) =
            serde_json::to_value(self).expect("a record serializes to JSON")
        else {
            unreachable!("a record serializes to a JSON object");
        };
        let sorted: BTreeMap<String, serde_json::Value> =
            fields.into_iter().filter(|(key, _)| key != "sig").collect();
        serde_json::to_vec(&sorted).expect("a JSON map serializes")
    }

    /// The node this record judges.
    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The content the judgement is bound to.
    #[must_use]
    pub fn view_hash(&self) -> ViewHash {
        self.view_hash
    }

    /// The judgement itself: an approval, or a decline vetoing the node.
    #[must_use]
    pub fn label(&self) -> Label {
        self.label
    }

    /// Mandatory on a decline; optional context on an approval.
    #[must_use]
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    /// Who made the judgement; also names the storage file.
    #[must_use]
    pub fn approver(&self) -> &str {
        &self.approver
    }

    /// Names the key that will sign once signatures land.
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Audit trail only: eval never reads it.
    #[must_use]
    pub fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    /// The current phase at approval time.
    #[must_use]
    pub fn phase(&self) -> &str {
        &self.phase
    }

    /// The approver-side reservation, if one was placed.
    #[must_use]
    pub fn until(&self) -> Option<&str> {
        self.until.as_deref()
    }

    /// The policy digest, kept as its `sha256:<hex>` spelling.
    ///
    /// Deliberately untyped: the policy layout has not landed, so the record
    /// commits only to the digest spelling, not to what it identifies.
    #[must_use]
    pub fn policy_hash(&self) -> Option<&str> {
        self.policy_hash.as_deref()
    }
}

/// The raw shape a stored line deserializes to before validation.
///
/// Unknown fields are rejected here: a field this version does not know is a
/// record it does not understand, not noise to skip.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecord {
    v: u64,
    target: String,
    view_hash: String,
    label: Label,
    comment: Option<String>,
    approver: String,
    key_id: String,
    timestamp: String,
    phase: String,
    until: Option<String>,
    policy_hash: Option<String>,
    sig: String,
}

impl TryFrom<RawRecord> for Record {
    type Error = RecordError;

    fn try_from(raw: RawRecord) -> Result<Self, Self::Error> {
        // Version first: a record from another version should say so, not
        // stumble over a field spelling this layout cannot read.
        if raw.v != RECORD_VERSION {
            return Err(RecordError::UnsupportedVersion { found: raw.v });
        }
        let record = Self {
            v: raw.v,
            target: raw.target.parse()?,
            view_hash: raw.view_hash.parse()?,
            label: raw.label,
            comment: raw.comment,
            approver: raw.approver,
            key_id: raw.key_id,
            timestamp: raw.timestamp.parse()?,
            phase: raw.phase,
            until: raw.until,
            policy_hash: raw.policy_hash,
            sig: raw.sig,
        };
        record.validate()?;
        Ok(record)
    }
}

impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawRecord::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

/// A record that breaks a v1 invariant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    #[error("record version {found} is not supported; this build reads v1")]
    UnsupportedVersion { found: u64 },
    #[error(transparent)]
    Target(#[from] InvalidTarget),
    #[error(transparent)]
    Digest(#[from] InvalidDigest),
    #[error(transparent)]
    Timestamp(#[from] InvalidTimestamp),
    #[error("a decline is a veto and must say why; `comment` is required")]
    MissingDeclineComment,
    #[error("a comment must not be blank; omit it instead")]
    BlankComment,
    #[error("approver `{0}` must be non-empty and usable as a file name")]
    InvalidApprover(String),
    #[error("key_id must not be blank")]
    BlankKeyId,
    #[error("phase must not be blank")]
    BlankPhase,
    #[error("until must not be blank when present; omit it instead")]
    BlankUntil,
    #[error("{field} must not contain control characters")]
    ControlCharacters { field: &'static str },
    #[error("`sig` is reserved and must stay empty until signatures land")]
    ReservedSignature,
}

/// Whether `name` can name the `approvals/<name>.jsonl` file.
///
/// Storage derives the file from the approver, so the name is held to an
/// allow-list — ASCII letters, digits, `_`, `-`, `.` — rather than a list of
/// known-bad characters: anything a filesystem might reinterpret (separators,
/// traversal, drive prefixes, alternate data streams, control or bidi
/// characters) is rejected when the record is built, not when a path breaks.
fn is_safe_file_stem(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !is_windows_reserved(name)
}

/// A name Windows maps to a device whatever its extension.
///
/// Opening `NUL.jsonl` for append reports success while persisting nothing,
/// which is exactly the silent record loss this module exists to prevent.
fn is_windows_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => match upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
        {
            Some(digit) => digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()),
            None => false,
        },
    }
}

/// Whether `text` carries a control character the field may not.
///
/// A stored line always escapes them, so they cannot corrupt the jsonl
/// format; they are rejected so a later consumer printing a field to a
/// terminal or a log cannot be fed ANSI escapes or spoofed line breaks.
fn has_forbidden_control(text: &str, allow_line_breaks: bool) -> bool {
    text.chars()
        .any(|c| c.is_control() && !(allow_line_breaks && matches!(c, '\n' | '\t')))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW_HASH: &str =
        "sha256:59481b66866b8ca5f21be852caa2f692a757841d5f58221fa189b2e12a6b42a5";

    fn fields() -> RecordFields {
        RecordFields {
            target: Target::new(Mode::Fiat, "mass_margin").expect("a valid target"),
            view_hash: VIEW_HASH.parse().expect("a valid digest"),
            approver: "iwata".to_owned(),
            key_id: "iwata-2026".to_owned(),
            timestamp: "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
            phase: "M3".to_owned(),
            until: None,
            policy_hash: None,
        }
    }

    #[test]
    fn a_target_round_trips_through_its_displayed_form() {
        // Arrange
        let original = Target::new(Mode::Bind, "power::margin").expect("a valid target");

        // Act
        let parsed: Target = original.to_string().parse().expect("the spelling parses");

        // Assert
        assert_eq!(parsed, original);
        assert_eq!(original.to_string(), "bind:power::margin");
    }

    #[test]
    fn rejects_a_target_that_is_not_kind_colon_id() {
        for text in [
            "margin",
            "gate:margin",
            "bind:",
            "bind:3margin",
            "bind:power::",
            "bind:power::::margin",
            "Bind:margin",
        ] {
            assert!(text.parse::<Target>().is_err(), "`{text}` should not parse");
        }
    }

    #[test]
    fn accepts_the_stored_timestamp_spelling_only() {
        for text in [
            "2026-09-10T08:30:00Z",
            "2026-09-10T08:30:00.250Z",
            "2024-02-29T23:59:60Z",
        ] {
            assert!(text.parse::<Timestamp>().is_ok(), "`{text}` should parse");
        }
        for text in [
            "2026-09-10 08:30:00Z",
            "2026-09-10T08:30:00",
            "2026-09-10T08:30:00+00:00",
            "2026-09-10t08:30:00z",
            "2026-13-10T08:30:00Z",
            "2026-02-30T08:30:00Z",
            "2023-02-29T08:30:00Z",
            "2026-09-10T24:00:00Z",
            "2026-09-10T08:30:00.Z",
        ] {
            assert!(
                text.parse::<Timestamp>().is_err(),
                "`{text}` should not parse"
            );
        }
    }

    #[test]
    fn a_decline_requires_a_comment() {
        // Arrange & Act: build the same decline with and without its reason.
        let refused = Record::new(Label::Decline, None, fields());
        let recorded = Record::decline(fields(), "the margin math is wrong".to_owned());

        // Assert
        assert_eq!(refused, Err(RecordError::MissingDeclineComment));
        assert!(recorded.is_ok());
    }

    #[test]
    fn a_blank_comment_is_not_a_comment() {
        let approval = Record::approve(fields(), Some("   ".to_owned()));

        assert_eq!(approval, Err(RecordError::BlankComment));
    }

    #[test]
    fn an_approver_that_cannot_name_a_file_is_rejected() {
        for approver in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a\nb",
            // A drive prefix or an NTFS alternate data stream on Windows.
            "C:evil",
            "iwata:hidden",
            // Windows device names persist nothing while reporting success.
            "NUL",
            "con",
            "com1.log",
            // A bidi override can spoof the directory listing.
            "a\u{202E}b",
            "a b",
        ] {
            // Arrange
            let mut fields = fields();
            fields.approver = approver.to_owned();

            // Act & Assert
            assert_eq!(
                Record::approve(fields, None),
                Err(RecordError::InvalidApprover(approver.to_owned())),
                "`{approver}` should be rejected"
            );
        }
    }

    #[test]
    fn a_comment_may_break_lines_but_not_carry_other_controls() {
        // Arrange & Act: prose line breaks are content; an ANSI escape that
        // could rewrite a terminal later is not.
        let prose = Record::approve(fields(), Some("first line\n\tsecond line".to_owned()));
        let escape = Record::approve(fields(), Some("looks fine\u{1b}[2K".to_owned()));

        // Assert
        assert!(prose.is_ok());
        assert_eq!(
            escape,
            Err(RecordError::ControlCharacters { field: "comment" })
        );
    }

    #[test]
    fn single_valued_fields_reject_every_control_character() {
        for (field, mutate) in [
            (
                "key_id",
                (|f: &mut RecordFields| f.key_id = "key\nid".to_owned()) as fn(&mut RecordFields),
            ),
            ("phase", |f| f.phase = "M3\u{1b}[2K".to_owned()),
            ("until", |f| f.until = Some("FM\tlater".to_owned())),
        ] {
            // Arrange
            let mut fields = fields();
            mutate(&mut fields);

            // Act & Assert
            assert_eq!(
                Record::approve(fields, None),
                Err(RecordError::ControlCharacters { field }),
                "`{field}` should reject control characters"
            );
        }
    }

    #[test]
    fn a_present_policy_hash_must_be_a_digest() {
        // Arrange
        let mut fields = fields();
        fields.policy_hash = Some("not-a-digest".to_owned());

        // Act
        let error = Record::approve(fields, None).expect_err("the policy hash is malformed");

        // Assert
        assert_eq!(
            error,
            RecordError::Digest(InvalidDigest("not-a-digest".to_owned()))
        );
    }

    #[test]
    fn canonical_bytes_sort_keys_and_drop_sig() {
        // Arrange
        let record = Record::approve(fields(), None).expect("a valid record");

        // Act
        let canonical = String::from_utf8(record.canonical_bytes()).expect("JSON is UTF-8");

        // Assert: alphabetical keys, no `sig`, absent optionals as null.
        assert_eq!(
            canonical,
            format!(
                "{{\"approver\":\"iwata\",\"comment\":null,\"key_id\":\"iwata-2026\",\
                 \"label\":\"approve\",\"phase\":\"M3\",\"policy_hash\":null,\
                 \"target\":\"fiat:mass_margin\",\"timestamp\":\"2026-09-10T08:30:00Z\",\
                 \"until\":null,\"v\":1,\"view_hash\":\"{VIEW_HASH}\"}}"
            )
        );
    }

    #[test]
    fn storage_lines_keep_the_spec_field_order() {
        // Arrange
        let record = Record::approve(fields(), None).expect("a valid record");

        // Act
        let line = serde_json::to_string(&record).expect("a record serializes");

        // Assert: the spec's listing order, `sig` last and empty.
        assert_eq!(
            line,
            format!(
                "{{\"v\":1,\"target\":\"fiat:mass_margin\",\"view_hash\":\"{VIEW_HASH}\",\
                 \"label\":\"approve\",\"comment\":null,\"approver\":\"iwata\",\
                 \"key_id\":\"iwata-2026\",\"timestamp\":\"2026-09-10T08:30:00Z\",\
                 \"phase\":\"M3\",\"until\":null,\"policy_hash\":null,\"sig\":\"\"}}"
            )
        );
    }

    #[test]
    fn a_record_round_trips_through_its_stored_line() {
        // Arrange
        let original = Record::decline(fields(), "the margin math is wrong".to_owned())
            .expect("a valid record");

        // Act
        let line = serde_json::to_string(&original).expect("a record serializes");
        let parsed: Record = serde_json::from_str(&line).expect("the stored line parses");

        // Assert
        assert_eq!(parsed, original);
    }

    #[test]
    fn rejects_a_stored_line_with_an_unknown_field() {
        // Arrange
        let record = Record::approve(fields(), None).expect("a valid record");
        let mut value = serde_json::to_value(&record).expect("a record serializes");
        value["mood"] = serde_json::Value::String("optimistic".to_owned());

        // Act
        let error = serde_json::from_value::<Record>(value).expect_err("unknown fields are fatal");

        // Assert
        assert!(error.to_string().contains("mood"), "{error}");
    }

    #[test]
    fn rejects_a_stored_line_from_another_version() {
        // Arrange
        let record = Record::approve(fields(), None).expect("a valid record");
        let mut value = serde_json::to_value(&record).expect("a record serializes");
        value["v"] = serde_json::Value::from(2);
        // A target spelling v1 cannot read: the version must win the race.
        value["target"] = serde_json::Value::String("gate:margin".to_owned());

        // Act
        let error = serde_json::from_value::<Record>(value).expect_err("v2 is not this layout");

        // Assert
        assert!(error.to_string().contains("version 2"), "{error}");
    }

    #[test]
    fn rejects_a_stored_line_carrying_a_signature() {
        // Arrange
        let record = Record::approve(fields(), None).expect("a valid record");
        let mut value = serde_json::to_value(&record).expect("a record serializes");
        value["sig"] = serde_json::Value::String("deadbeef".to_owned());

        // Act
        let error =
            serde_json::from_value::<Record>(value).expect_err("signatures have not landed");

        // Assert
        assert!(error.to_string().contains("reserved"), "{error}");
    }

    #[test]
    fn accepts_a_stored_line_that_omits_absent_optionals() {
        // Arrange: hand-written storage may drop the null fields entirely.
        let line = format!(
            "{{\"v\":1,\"target\":\"fiat:mass_margin\",\"view_hash\":\"{VIEW_HASH}\",\
             \"label\":\"approve\",\"approver\":\"iwata\",\"key_id\":\"iwata-2026\",\
             \"timestamp\":\"2026-09-10T08:30:00Z\",\"phase\":\"M3\",\"sig\":\"\"}}"
        );

        // Act
        let parsed: Record = serde_json::from_str(&line).expect("omitted optionals read as null");

        // Assert
        assert_eq!(
            parsed,
            Record::approve(fields(), None).expect("a valid record")
        );
    }
}
