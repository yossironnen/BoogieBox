//! Smart playlists: the rule definition, its validator, the SQL compiler and the evaluator, plus the schema
//! (definitions, revision counters, `tracks.added_at`, BoogieMix input snapshots). Plan:
//! `wip/smart-playlists-implementation-plan.md` §4 and §2a. Rules compile to fixed SQL fragments chosen from
//! allowlists; every value is a bound parameter.

use crate::music::EntityId;
use crate::playlists::{self, CreatePlaylistError, PlaylistRow, PlaylistTrackRow};
use rusqlite::{types::Value, Connection, ErrorCode, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::sync::mpsc;
use std::time::Duration;

/// The only definition schema this build reads and writes.
pub const SCHEMA_VERSION: u32 = 1;
/// Largest serialized definition accepted.
pub const MAX_DEFINITION_BYTES: usize = 16 * 1024;
/// Deepest group nesting, counting the root as 1.
pub const MAX_DEPTH: usize = 3;
/// Most rules (leaves) in one definition.
pub const MAX_RULES: usize = 24;
/// Most nodes (groups and rules, root included).
pub const MAX_NODES: usize = 32;
/// Most libraries one library rule may name.
pub const MAX_LIBRARY_IDS: usize = 100;
/// Longest text value, in characters after trimming.
pub const MAX_TEXT_CHARS: usize = 128;
/// Track cap when the definition gives none.
pub const DEFAULT_MAX_TRACKS: u32 = 100;
/// Highest track cap.
pub const MAX_TRACKS: u32 = 1000;
/// Highest total-length cap (24 h).
pub const MAX_DURATION_SECONDS: u32 = 86_400;
/// Longest relative-date window, in days.
pub const MAX_DAYS: i64 = 3650;
/// Most sort keys.
pub const MAX_SORT_KEYS: usize = 2;
/// How long one evaluation may run before SQLite is interrupted.
pub const DEFAULT_QUERY_BUDGET: Duration = Duration::from_secs(1);

// ── Definition (API / stored shape) ────────────────────────────────────────────

/// A smart playlist definition as sent by clients and stored (canonical JSON after validation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u32,
    pub root: Group,
    #[serde(default)]
    pub sort: Vec<SortKey>,
    #[serde(default)]
    pub limit: Limit,
}

/// A rule group: every child (`all`) or at least one child (`any`) must match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Group {
    pub mode: Mode,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    All,
    Any,
}

/// One child of a group: a nested group or a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Group(Group),
    Rule(Rule),
}

/// One rule. Field and operator stay strings here so the validator can name what is wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rule {
    pub field: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Json>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SortKey {
    pub field: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limit {
    #[serde(default = "default_max_tracks")]
    pub max_tracks: u32,
    #[serde(default)]
    pub max_duration_seconds: Option<u32>,
}

impl Default for Limit {
    fn default() -> Self {
        Self {
            max_tracks: DEFAULT_MAX_TRACKS,
            max_duration_seconds: None,
        }
    }
}

fn default_max_tracks() -> u32 {
    DEFAULT_MAX_TRACKS
}

// ── Validation errors ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleErrorCode {
    /// Not a definition at all (bad JSON, wrong shape, unknown property).
    Invalid,
    TooLarge,
    UnsupportedVersion,
    TooDeep,
    TooManyRules,
    TooManyNodes,
    EmptyGroup,
    UnknownField,
    /// The operator is unknown or not allowed for the field.
    UnknownOperator,
    MissingValue,
    UnexpectedValue,
    BadValue,
    /// A between range whose first value is above the second.
    BadRange,
    BadSort,
    BadLimit,
}

/// What is wrong and where (`root.children[1].value`, `sort[0]`, `limit.maxTracks`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleError {
    pub path: String,
    pub code: RuleErrorCode,
}

fn err(path: impl Into<String>, code: RuleErrorCode) -> RuleError {
    RuleError {
        path: path.into(),
        code,
    }
}

// ── Validated form ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextField {
    Artist,
    Album,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextOp {
    Is,
    IsNot,
    Contains,
    NotContains,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumField {
    Year,
    Rating,
    PlayCount,
    Bpm,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Cmp {
    Eq(f64),
    Gte(f64),
    Lte(f64),
    Between(f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateField {
    Added,
    LastPlayed,
}

#[derive(Debug, Clone, PartialEq)]
enum DateCmp {
    InLastDays(i64),
    NotInLastDays(i64),
    Before(String),
    OnOrAfter(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Condition {
    Text(TextField, TextOp, String),
    Genre {
        negate: bool,
        label: String,
    },
    GenrePresent(bool),
    Number(NumField, Cmp),
    /// Year, rating or BPM known (`true`) or unknown.
    NumberPresent(NumField, bool),
    Library {
        include: bool,
        ids: Vec<String>,
    },
    Date(DateField, DateCmp),
    /// Played at least once (`true`) or never.
    PlayedPresent(bool),
    Favourite(bool),
}

#[derive(Debug, Clone, PartialEq)]
enum CNode {
    Group(CGroup),
    Cond(Condition),
}

#[derive(Debug, Clone, PartialEq)]
struct CGroup {
    mode: Mode,
    children: Vec<CNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortField {
    Title,
    Artist,
    Album,
    Year,
    Rating,
    AddedAt,
    LastPlayedAt,
    PlayCount,
    Bpm,
}

/// A definition that passed validation, with its canonical form and compiled rule tree.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidDefinition {
    canonical: Definition,
    root: CGroup,
    sort: Vec<(SortField, Direction)>,
    max_tracks: u32,
    max_duration_seconds: Option<u32>,
}

impl ValidDefinition {
    /// The normalized definition (trimmed text, de-duplicated library ids).
    pub fn definition(&self) -> &Definition {
        &self.canonical
    }

    /// Canonical JSON for storage.
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(&self.canonical).unwrap_or_default()
    }

    /// Whether membership can change with time alone (an "in the last / not in the last N days" rule), so a
    /// cached result must expire even when no revision moved.
    pub fn has_relative_dates(&self) -> bool {
        fn walk(group: &CGroup) -> bool {
            group.children.iter().any(|child| match child {
                CNode::Group(g) => walk(g),
                CNode::Cond(Condition::Date(_, DateCmp::InLastDays(_)))
                | CNode::Cond(Condition::Date(_, DateCmp::NotInLastDays(_))) => true,
                CNode::Cond(_) => false,
            })
        }
        walk(&self.root)
    }
}

/// Parses and validates a definition from JSON text.
pub fn parse_definition(json: &str) -> Result<ValidDefinition, Vec<RuleError>> {
    if json.len() > MAX_DEFINITION_BYTES {
        return Err(vec![err("", RuleErrorCode::TooLarge)]);
    }
    let def: Definition =
        serde_json::from_str(json).map_err(|_| vec![err("", RuleErrorCode::Invalid)])?;
    validate(def)
}

/// Validates a definition, returning every problem found.
pub fn validate(mut def: Definition) -> Result<ValidDefinition, Vec<RuleError>> {
    if def.schema_version != SCHEMA_VERSION {
        return Err(vec![err(
            "schemaVersion",
            RuleErrorCode::UnsupportedVersion,
        )]);
    }
    let serialized = serde_json::to_string(&def).unwrap_or_default();
    if serialized.len() > MAX_DEFINITION_BYTES {
        return Err(vec![err("", RuleErrorCode::TooLarge)]);
    }

    let mut errors = Vec::new();
    let mut counts = Counts::default();
    let root = check_group(&mut def.root, "root", 1, &mut counts, &mut errors);
    if counts.rules > MAX_RULES {
        errors.push(err("root", RuleErrorCode::TooManyRules));
    }
    if counts.nodes > MAX_NODES {
        errors.push(err("root", RuleErrorCode::TooManyNodes));
    }

    let mut sort = Vec::new();
    if def.sort.len() > MAX_SORT_KEYS {
        errors.push(err("sort", RuleErrorCode::BadSort));
    }
    for (i, key) in def.sort.iter().enumerate() {
        match sort_field(&key.field) {
            Some(field) if !sort.iter().any(|(f, _)| *f == field) => {
                sort.push((field, key.direction))
            }
            _ => errors.push(err(format!("sort[{i}]"), RuleErrorCode::BadSort)),
        }
    }

    if !(1..=MAX_TRACKS).contains(&def.limit.max_tracks) {
        errors.push(err("limit.maxTracks", RuleErrorCode::BadLimit));
    }
    if let Some(seconds) = def.limit.max_duration_seconds {
        if !(1..=MAX_DURATION_SECONDS).contains(&seconds) {
            errors.push(err("limit.maxDurationSeconds", RuleErrorCode::BadLimit));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    if sort.is_empty() {
        sort.push((SortField::Title, Direction::Asc));
    }
    Ok(ValidDefinition {
        max_tracks: def.limit.max_tracks,
        max_duration_seconds: def.limit.max_duration_seconds,
        root,
        sort,
        canonical: def,
    })
}

#[derive(Default)]
struct Counts {
    rules: usize,
    nodes: usize,
}

fn check_group(
    group: &mut Group,
    path: &str,
    depth: usize,
    counts: &mut Counts,
    errors: &mut Vec<RuleError>,
) -> CGroup {
    counts.nodes += 1;
    if depth > MAX_DEPTH {
        errors.push(err(path, RuleErrorCode::TooDeep));
    }
    if group.children.is_empty() {
        errors.push(err(path, RuleErrorCode::EmptyGroup));
    }
    let mut children = Vec::new();
    for (i, child) in group.children.iter_mut().enumerate() {
        let child_path = format!("{path}.children[{i}]");
        match child {
            Node::Group(g) => {
                children.push(CNode::Group(check_group(
                    g,
                    &child_path,
                    depth + 1,
                    counts,
                    errors,
                )));
            }
            Node::Rule(rule) => {
                counts.nodes += 1;
                counts.rules += 1;
                if let Some(cond) = check_rule(rule, &child_path, errors) {
                    children.push(CNode::Cond(cond));
                }
            }
        }
    }
    CGroup {
        mode: group.mode,
        children,
    }
}

/// Operators each field allows; anything else is `UnknownOperator`.
fn allowed_operators(field: &str) -> Option<&'static [&'static str]> {
    const TEXT: &[&str] = &["is", "is_not", "contains", "not_contains"];
    const KNOWN_NUMBER: &[&str] = &["is", "gte", "lte", "between", "is_missing", "is_present"];
    Some(match field {
        "artist" | "album" => TEXT,
        "genre" => &["is", "is_not", "is_missing", "is_present"],
        "year" | "rating" | "bpm" => KNOWN_NUMBER,
        "play_count" => &["is", "gte", "lte", "between"],
        "library_id" => &["in", "not_in"],
        "added_at" => &["in_last_days", "not_in_last_days", "before", "on_or_after"],
        "last_played_at" => &[
            "in_last_days",
            "not_in_last_days",
            "before",
            "on_or_after",
            "is_missing",
            "is_present",
        ],
        "favourite" => &["is", "is_not"],
        _ => return None,
    })
}

fn check_rule(rule: &mut Rule, path: &str, errors: &mut Vec<RuleError>) -> Option<Condition> {
    let Some(ops) = allowed_operators(&rule.field) else {
        errors.push(err(format!("{path}.field"), RuleErrorCode::UnknownField));
        return None;
    };
    if !ops.contains(&rule.operator.as_str()) {
        errors.push(err(
            format!("{path}.operator"),
            RuleErrorCode::UnknownOperator,
        ));
        return None;
    }
    let op = rule.operator.as_str();
    let value_path = format!("{path}.value");
    let takes_value = !matches!(op, "is_missing" | "is_present") && rule.field != "favourite";
    match (&rule.value, takes_value) {
        (Some(_), false) => {
            errors.push(err(value_path, RuleErrorCode::UnexpectedValue));
            return None;
        }
        (None, true) => {
            errors.push(err(value_path, RuleErrorCode::MissingValue));
            return None;
        }
        _ => {}
    }

    let bad = |errors: &mut Vec<RuleError>, code| -> Option<Condition> {
        errors.push(err(format!("{path}.value"), code));
        None
    };
    let present = op == "is_present";
    let field = rule.field.clone();
    match field.as_str() {
        "artist" | "album" => {
            let Some(text) = text_value(rule.value.as_ref()) else {
                return bad(errors, RuleErrorCode::BadValue);
            };
            rule.value = Some(Json::String(text.clone()));
            let text_field = if field == "artist" {
                TextField::Artist
            } else {
                TextField::Album
            };
            let text_op = match op {
                "is" => TextOp::Is,
                "is_not" => TextOp::IsNot,
                "contains" => TextOp::Contains,
                _ => TextOp::NotContains,
            };
            Some(Condition::Text(text_field, text_op, text))
        }
        "genre" => match op {
            "is_missing" | "is_present" => Some(Condition::GenrePresent(present)),
            _ => {
                let Some(label) = text_value(rule.value.as_ref()) else {
                    return bad(errors, RuleErrorCode::BadValue);
                };
                rule.value = Some(Json::String(label.clone()));
                Some(Condition::Genre {
                    negate: op == "is_not",
                    label,
                })
            }
        },
        "year" | "rating" | "bpm" | "play_count" => {
            let num_field = match field.as_str() {
                "year" => NumField::Year,
                "rating" => NumField::Rating,
                "bpm" => NumField::Bpm,
                _ => NumField::PlayCount,
            };
            if matches!(op, "is_missing" | "is_present") {
                return Some(Condition::NumberPresent(num_field, present));
            }
            let value = rule.value.as_ref()?;
            let cmp = if op == "between" {
                let Some(pair) = value.as_array().filter(|v| v.len() == 2) else {
                    return bad(errors, RuleErrorCode::BadValue);
                };
                let (Some(a), Some(b)) = (number(num_field, &pair[0]), number(num_field, &pair[1]))
                else {
                    return bad(errors, RuleErrorCode::BadValue);
                };
                if a > b {
                    return bad(errors, RuleErrorCode::BadRange);
                }
                Cmp::Between(a, b)
            } else {
                let Some(n) = number(num_field, value) else {
                    return bad(errors, RuleErrorCode::BadValue);
                };
                match op {
                    "is" => Cmp::Eq(n),
                    "gte" => Cmp::Gte(n),
                    _ => Cmp::Lte(n),
                }
            };
            Some(Condition::Number(num_field, cmp))
        }
        "library_id" => {
            let Some(items) = rule.value.as_ref().and_then(Json::as_array) else {
                return bad(errors, RuleErrorCode::BadValue);
            };
            let mut ids: Vec<String> = Vec::new();
            for item in items {
                let id = match item {
                    Json::String(s) => s.trim().to_owned(),
                    Json::Number(n) if n.is_i64() => n.to_string(),
                    _ => return bad(errors, RuleErrorCode::BadValue),
                };
                if id.is_empty() || id.chars().count() > MAX_TEXT_CHARS {
                    return bad(errors, RuleErrorCode::BadValue);
                }
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if ids.is_empty() || ids.len() > MAX_LIBRARY_IDS {
                return bad(errors, RuleErrorCode::BadValue);
            }
            rule.value = Some(Json::Array(ids.iter().cloned().map(Json::String).collect()));
            Some(Condition::Library {
                include: op == "in",
                ids,
            })
        }
        "added_at" | "last_played_at" => {
            let date_field = if field == "added_at" {
                DateField::Added
            } else {
                DateField::LastPlayed
            };
            if matches!(op, "is_missing" | "is_present") {
                return Some(Condition::PlayedPresent(present));
            }
            let value = rule.value.as_ref()?;
            let cmp = match op {
                "in_last_days" | "not_in_last_days" => {
                    let Some(days) = whole(value).filter(|d| (1..=MAX_DAYS).contains(d)) else {
                        return bad(errors, RuleErrorCode::BadValue);
                    };
                    if op == "in_last_days" {
                        DateCmp::InLastDays(days)
                    } else {
                        DateCmp::NotInLastDays(days)
                    }
                }
                _ => {
                    let Some(date) = value.as_str().filter(|s| is_calendar_date(s)) else {
                        return bad(errors, RuleErrorCode::BadValue);
                    };
                    if op == "before" {
                        DateCmp::Before(date.to_owned())
                    } else {
                        DateCmp::OnOrAfter(date.to_owned())
                    }
                }
            };
            Some(Condition::Date(date_field, cmp))
        }
        _ => Some(Condition::Favourite(op == "is")),
    }
}

fn text_value(value: Option<&Json>) -> Option<String> {
    let text = value?.as_str()?.trim();
    let chars = text.chars().count();
    (1..=MAX_TEXT_CHARS)
        .contains(&chars)
        .then(|| text.to_owned())
}

/// A whole number, accepting `4` and `4.0`.
fn whole(value: &Json) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(n);
    }
    let f = value.as_f64()?;
    (f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15).then_some(f as i64)
}

fn number(field: NumField, value: &Json) -> Option<f64> {
    match field {
        NumField::Year => whole(value)
            .filter(|y| (1..=9999).contains(y))
            .map(|y| y as f64),
        NumField::PlayCount => whole(value)
            .filter(|n| (0..=i64::from(i32::MAX)).contains(n))
            .map(|n| n as f64),
        NumField::Rating => value
            .as_f64()
            .filter(|r| (0.5..=5.0).contains(r) && (r * 2.0).fract() == 0.0),
        NumField::Bpm => value
            .as_f64()
            .filter(|b| b.is_finite() && *b > 0.0 && *b <= 1000.0),
    }
}

/// `YYYY-MM-DD` naming a real calendar day.
fn is_calendar_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &s[r];
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1 && (1..=days_in_month).contains(&day)
}

fn sort_field(name: &str) -> Option<SortField> {
    Some(match name {
        "title" => SortField::Title,
        "artist" => SortField::Artist,
        "album" => SortField::Album,
        "year" => SortField::Year,
        "rating" => SortField::Rating,
        "added_at" => SortField::AddedAt,
        "last_played_at" => SortField::LastPlayedAt,
        "play_count" => SortField::PlayCount,
        "bpm" => SortField::Bpm,
        _ => return None,
    })
}

// ── SQL compiler ───────────────────────────────────────────────────────────────

const ARTIST_NAME: &str = "TRIM(COALESCE(ar.name, ''))";
const ALBUM_ARTIST: &str = "TRIM(t.album_artist)";
const ALBUM_TITLE: &str = "TRIM(COALESCE(al.title, ''))";
const GENRE: &str = "TRIM(COALESCE(t.genre, ''))";
const BPM: &str =
    "(CASE WHEN t.bpm > 0 THEN t.bpm WHEN t.bpm_detected > 0 THEN t.bpm_detected END)";
const LAST_PLAYED: &str = "NULLIF(s.last_played_at, '')";
const PLAY_COUNT: &str = "COALESCE(s.play_count, 0)";

#[derive(Default)]
struct Compiler {
    params: Vec<Value>,
    owner_slot: Option<String>,
    now_slot: Option<String>,
    artists: bool,
    albums: bool,
    ratings: bool,
    stats: bool,
}

impl Compiler {
    fn bind(&mut self, value: impl Into<Value>) -> String {
        self.params.push(value.into());
        format!("?{}", self.params.len())
    }

    fn owner(&mut self, owner: &str) -> String {
        if self.owner_slot.is_none() {
            self.owner_slot = Some(self.bind(owner.to_owned()));
        }
        self.owner_slot.clone().unwrap_or_default()
    }

    fn now(&mut self, now: &str) -> String {
        if self.now_slot.is_none() {
            self.now_slot = Some(self.bind(now.to_owned()));
        }
        self.now_slot.clone().unwrap_or_default()
    }

    fn list(&mut self, values: &[String]) -> String {
        values
            .iter()
            .map(|v| self.bind(v.clone()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

struct Context<'a> {
    conn: &'a Connection,
    owner: &'a str,
    now: &'a str,
}

fn compile_group(c: &mut Compiler, ctx: &Context, group: &CGroup) -> rusqlite::Result<String> {
    let joiner = match group.mode {
        Mode::All => " AND ",
        Mode::Any => " OR ",
    };
    let mut parts = Vec::with_capacity(group.children.len());
    for child in &group.children {
        parts.push(match child {
            CNode::Group(g) => compile_group(c, ctx, g)?,
            CNode::Cond(cond) => format!("COALESCE(({}), 0)", compile_condition(c, ctx, cond)?),
        });
    }
    Ok(format!("({})", parts.join(joiner)))
}

fn compile_cmp(c: &mut Compiler, col: &str, cmp: Cmp, round: bool) -> String {
    match cmp {
        Cmp::Eq(v) if round => format!("ROUND({col}) = ROUND({})", c.bind(v)),
        Cmp::Eq(v) => format!("{col} = {}", c.bind(v)),
        Cmp::Gte(v) => format!("{col} >= {}", c.bind(v)),
        Cmp::Lte(v) => format!("{col} <= {}", c.bind(v)),
        Cmp::Between(a, b) => format!("{col} BETWEEN {} AND {}", c.bind(a), c.bind(b)),
    }
}

fn compile_condition(
    c: &mut Compiler,
    ctx: &Context,
    cond: &Condition,
) -> rusqlite::Result<String> {
    Ok(match cond {
        Condition::Text(field, op, value) => {
            let cols: &[&str] = match field {
                TextField::Artist => {
                    c.artists = true;
                    &[ARTIST_NAME, ALBUM_ARTIST]
                }
                TextField::Album => {
                    c.albums = true;
                    &[ALBUM_TITLE]
                }
            };
            let slot = c.bind(value.clone());
            let test = |col: &&str| match op {
                TextOp::Is | TextOp::IsNot => format!("{col} = {slot} COLLATE NOCASE"),
                TextOp::Contains | TextOp::NotContains => {
                    format!("instr(lower({col}), lower({slot})) > 0")
                }
            };
            let matched = cols.iter().map(test).collect::<Vec<_>>().join(" OR ");
            match op {
                TextOp::Is | TextOp::Contains => matched,
                TextOp::IsNot | TextOp::NotContains => {
                    let known = cols
                        .iter()
                        .map(|col| format!("{col} <> ''"))
                        .collect::<Vec<_>>()
                        .join(" OR ");
                    format!("({known}) AND NOT ({matched})")
                }
            }
        }
        Condition::Genre { negate, label } => {
            let stored = crate::subsonic::genre_values(ctx.conn, label)?;
            match (negate, stored.is_empty()) {
                (false, true) => "0".to_owned(),
                (false, false) => format!("t.genre IN ({})", c.list(&stored)),
                (true, true) => format!("{GENRE} <> ''"),
                (true, false) => format!("{GENRE} <> '' AND t.genre NOT IN ({})", c.list(&stored)),
            }
        }
        Condition::GenrePresent(present) => {
            format!("{GENRE} {} ''", if *present { "<>" } else { "=" })
        }
        Condition::Number(field, cmp) => match field {
            NumField::Year => format!("t.year > 0 AND {}", compile_cmp(c, "t.year", *cmp, false)),
            NumField::Rating => {
                c.ratings = true;
                compile_cmp(c, "r.rating", *cmp, false)
            }
            NumField::PlayCount => {
                c.stats = true;
                compile_cmp(c, PLAY_COUNT, *cmp, false)
            }
            NumField::Bpm => compile_cmp(c, BPM, *cmp, true),
        },
        Condition::NumberPresent(field, present) => {
            let col = match field {
                NumField::Year => "(CASE WHEN t.year > 0 THEN t.year END)",
                NumField::Rating => {
                    c.ratings = true;
                    "r.rating"
                }
                NumField::Bpm => BPM,
                NumField::PlayCount => PLAY_COUNT,
            };
            format!("{col} IS {}NULL", if *present { "NOT " } else { "" })
        }
        Condition::Library { include, ids } => format!(
            "t.library_id {}IN ({})",
            if *include { "" } else { "NOT " },
            c.list(ids)
        ),
        Condition::Date(field, cmp) => {
            let col = match field {
                DateField::Added => "t.added_at",
                DateField::LastPlayed => {
                    c.stats = true;
                    LAST_PLAYED
                }
            };
            match cmp {
                DateCmp::InLastDays(days) | DateCmp::NotInLastDays(days) => {
                    let now = c.now(ctx.now);
                    let modifier = c.bind(format!("-{days} days"));
                    let cutoff = format!("datetime({now}, {modifier})");
                    match (cmp, field) {
                        (DateCmp::InLastDays(_), _) => format!("{col} >= {cutoff}"),
                        (_, DateField::LastPlayed) => {
                            format!("({col} IS NULL OR {col} < {cutoff})")
                        }
                        _ => format!("{col} < {cutoff}"),
                    }
                }
                DateCmp::Before(date) => format!("{col} < datetime({})", c.bind(date.clone())),
                DateCmp::OnOrAfter(date) => {
                    format!("{col} >= datetime({})", c.bind(date.clone()))
                }
            }
        }
        Condition::PlayedPresent(present) => {
            c.stats = true;
            format!(
                "{LAST_PLAYED} IS {}NULL",
                if *present { "NOT " } else { "" }
            )
        }
        Condition::Favourite(is) => {
            let owner = c.owner(ctx.owner);
            format!(
                "{}EXISTS (SELECT 1 FROM subsonic_stars st WHERE st.user_id = {owner} AND st.item_type = 'track' AND st.item_id = t.id)",
                if *is { "" } else { "NOT " }
            )
        }
    })
}

fn sort_sql(c: &mut Compiler, field: SortField) -> (&'static str, bool) {
    match field {
        SortField::Title => ("t.title", true),
        SortField::Artist => {
            c.artists = true;
            (
                "NULLIF(COALESCE(NULLIF(TRIM(ar.name), ''), TRIM(t.album_artist)), '')",
                true,
            )
        }
        SortField::Album => {
            c.albums = true;
            ("NULLIF(TRIM(al.title), '')", true)
        }
        SortField::Year => ("(CASE WHEN t.year > 0 THEN t.year END)", false),
        SortField::Rating => {
            c.ratings = true;
            ("r.rating", false)
        }
        SortField::AddedAt => ("t.added_at", false),
        SortField::LastPlayedAt => {
            c.stats = true;
            (LAST_PLAYED, false)
        }
        SortField::PlayCount => {
            c.stats = true;
            (PLAY_COUNT, false)
        }
        SortField::Bpm => (BPM, false),
    }
}

/// The selection query and its parameters.
fn build_query(ctx: &Context, def: &ValidDefinition) -> rusqlite::Result<(String, Vec<Value>)> {
    let mut c = Compiler::default();
    let mut predicate = compile_group(&mut c, ctx, &def.root)?;
    if def.max_duration_seconds.is_some() {
        predicate.push_str(" AND t.duration > 0");
    }
    let mut order = Vec::new();
    for (field, direction) in &def.sort {
        let (expr, nocase) = sort_sql(&mut c, *field);
        let dir = match direction {
            Direction::Asc => "ASC",
            Direction::Desc => "DESC",
        };
        let collate = if nocase { " COLLATE NOCASE" } else { "" };
        order.push(format!("({expr}) IS NULL, {expr}{collate} {dir}"));
    }
    order.push("t.id ASC".to_owned());

    let mut joins = String::new();
    if c.artists {
        joins.push_str(" LEFT JOIN artists ar ON ar.id = t.artist_id");
    }
    if c.albums {
        joins.push_str(" LEFT JOIN albums al ON al.id = t.album_id");
    }
    if c.ratings {
        let owner = c.owner(ctx.owner);
        joins.push_str(&format!(
            " LEFT JOIN track_ratings r ON r.user_id = {owner} AND r.track_id = t.id"
        ));
    }
    if c.stats {
        let owner = c.owner(ctx.owner);
        joins.push_str(&format!(
            " LEFT JOIN user_track_stats s ON s.user_id = {owner} AND s.track_id = t.id"
        ));
    }
    let limit = c.bind(i64::from(def.max_tracks));
    let sql = format!(
        "SELECT t.id, t.duration FROM tracks t{joins} WHERE {predicate} ORDER BY {} LIMIT {limit}",
        order.join(", ")
    );
    Ok((sql, c.params))
}

// ── Evaluation ─────────────────────────────────────────────────────────────────

/// The tracks a definition selects, in playlist order, after both limits.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    pub track_ids: Vec<String>,
    pub total_duration: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("smart playlist evaluation took too long")]
    TimedOut,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// SQLite's current UTC time (`YYYY-MM-DD HH:MM:SS`), the evaluation clock for one evaluation.
pub fn db_now(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT datetime('now')", [], |r| r.get(0))
}

/// Evaluates `def` for `owner_id` at `now` (from [`db_now`]), with the default query budget.
pub fn evaluate(
    conn: &Connection,
    owner_id: &str,
    def: &ValidDefinition,
    now: &str,
) -> Result<Selection, EvalError> {
    evaluate_with_budget(conn, owner_id, def, now, DEFAULT_QUERY_BUDGET)
}

/// Evaluates `def`, interrupting SQLite if it runs past `budget`.
pub fn evaluate_with_budget(
    conn: &Connection,
    owner_id: &str,
    def: &ValidDefinition,
    now: &str,
    budget: Duration,
) -> Result<Selection, EvalError> {
    let ctx = Context {
        conn,
        owner: owner_id,
        now,
    };
    let rows = with_query_budget(conn, budget, || {
        let (sql, params) = build_query(&ctx, def)?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            let id = match row.get::<_, Value>(0)? {
                Value::Integer(n) => n.to_string(),
                Value::Text(s) => s,
                _ => String::new(),
            };
            Ok((id, row.get::<_, Option<f64>>(1)?.unwrap_or(0.0)))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let budget_seconds = def.max_duration_seconds.map(f64::from);
    let mut selection = Selection::default();
    for (id, duration) in rows {
        if let Some(limit) = budget_seconds {
            if selection.total_duration + duration > limit {
                break;
            }
        }
        selection.total_duration += duration.max(0.0);
        selection.track_ids.push(id);
    }
    Ok(selection)
}

/// Runs `work`, interrupting the connection if it is still running after `budget`. The watchdog thread is
/// joined before returning, so a late interrupt can only land while `work`'s statements are active (SQLite
/// clears the flag when the next statement starts with none active).
pub(crate) fn with_query_budget<T>(
    conn: &Connection,
    budget: Duration,
    work: impl FnOnce() -> rusqlite::Result<T>,
) -> Result<T, EvalError> {
    let handle = conn.get_interrupt_handle();
    let (done, finished) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if let Err(mpsc::RecvTimeoutError::Timeout) = finished.recv_timeout(budget) {
            handle.interrupt();
        }
    });
    let result = work();
    let _ = done.send(());
    let _ = watchdog.join();
    result.map_err(|e| match e.sqlite_error_code() {
        Some(ErrorCode::OperationInterrupted) => EvalError::TimedOut,
        _ => EvalError::Db(e),
    })
}

impl ValidDefinition {
    /// Whether any rule or sort key reads the owner's ratings, plays or favourites.
    fn uses_personal(&self) -> bool {
        fn walk(group: &CGroup) -> bool {
            group.children.iter().any(|child| match child {
                CNode::Group(g) => walk(g),
                CNode::Cond(cond) => matches!(
                    cond,
                    Condition::Number(NumField::Rating | NumField::PlayCount, _)
                        | Condition::NumberPresent(NumField::Rating, _)
                        | Condition::Date(DateField::LastPlayed, _)
                        | Condition::PlayedPresent(_)
                        | Condition::Favourite(_)
                ),
            })
        }
        walk(&self.root)
            || self.sort.iter().any(|(field, _)| {
                matches!(
                    field,
                    SortField::Rating | SortField::LastPlayedAt | SortField::PlayCount
                )
            })
    }

    /// Library ids named by library rules.
    fn library_ids(&self) -> Vec<String> {
        fn walk(group: &CGroup, out: &mut Vec<String>) {
            for child in &group.children {
                match child {
                    CNode::Group(g) => walk(g, out),
                    CNode::Cond(Condition::Library { ids, .. }) => {
                        for id in ids {
                            if !out.contains(id) {
                                out.push(id.clone());
                            }
                        }
                    }
                    CNode::Cond(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

/// Library ids the rules name that no longer exist (the rule then matches nothing for `in`).
pub fn missing_library_ids(
    conn: &Connection,
    def: &ValidDefinition,
) -> rusqlite::Result<Vec<String>> {
    let mut missing = Vec::new();
    for id in def.library_ids() {
        let exists = conn
            .query_row("SELECT 1 FROM libraries WHERE id = ?1", [&id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            missing.push(id);
        }
    }
    Ok(missing)
}

// ── Stored smart playlists ─────────────────────────────────────────────────────

/// Rows a preview returns.
pub const PREVIEW_SAMPLE: usize = 20;

#[derive(Debug)]
pub enum SmartError {
    NotFound,
    NameTaken,
    Invalid(Vec<RuleError>),
    /// The rules changed since the editor loaded them; carries the current edit revision.
    Conflict {
        edit_revision: i64,
    },
    /// Stored by a newer build: kept as it is, never evaluated or overwritten here.
    Unsupported,
    TimedOut,
    Db(rusqlite::Error),
}

impl From<rusqlite::Error> for SmartError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

impl From<EvalError> for SmartError {
    fn from(e: EvalError) -> Self {
        match e {
            EvalError::TimedOut => Self::TimedOut,
            EvalError::Db(e) => Self::Db(e),
        }
    }
}

impl From<CreatePlaylistError> for SmartError {
    fn from(e: CreatePlaylistError) -> Self {
        match e {
            CreatePlaylistError::NameTaken => Self::NameTaken,
            CreatePlaylistError::Db(e) => Self::Db(e),
        }
    }
}

struct Stored {
    json: String,
    schema_version: i64,
    edit_revision: i64,
    membership_revision: i64,
    evaluated_at: Option<String>,
    next_refresh_at: Option<String>,
    content_revision: Option<i64>,
    user_revision: Option<i64>,
}

impl Stored {
    fn parse(&self) -> Result<ValidDefinition, SmartError> {
        if self.schema_version != i64::from(SCHEMA_VERSION) {
            return Err(SmartError::Unsupported);
        }
        parse_definition(&self.json).map_err(|_| SmartError::Unsupported)
    }
}

/// The owner's smart definition for `playlist_id` (none for manual playlists and other users' playlists).
fn load_stored(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
) -> rusqlite::Result<Option<Stored>> {
    conn.query_row(
        "SELECT d.definition_json, d.schema_version, d.edit_revision, d.membership_revision,
                d.evaluated_at, d.next_refresh_at, d.content_revision, d.user_revision
         FROM smart_playlist_definitions d
         JOIN playlists p ON p.id = d.playlist_id
         WHERE d.playlist_id = ?1 AND p.user_id = ?2 AND p.kind = 'smart'",
        rusqlite::params![playlist_id, user_id],
        |r| {
            Ok(Stored {
                json: r.get(0)?,
                schema_version: r.get(1)?,
                edit_revision: r.get(2)?,
                membership_revision: r.get(3)?,
                evaluated_at: r.get(4)?,
                next_refresh_at: r.get(5)?,
                content_revision: r.get(6)?,
                user_revision: r.get(7)?,
            })
        },
    )
    .optional()
}

/// Runs `work` in a savepoint (nests inside an open transaction), rolling back on error.
fn in_savepoint<T, E: From<rusqlite::Error>>(
    conn: &Connection,
    work: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    conn.execute_batch("SAVEPOINT smart_playlist")?;
    match work() {
        Ok(value) => {
            conn.execute_batch("RELEASE smart_playlist")?;
            Ok(value)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO smart_playlist; RELEASE smart_playlist");
            Err(e)
        }
    }
}

fn text_of(value: Value) -> String {
    match value {
        Value::Integer(n) => n.to_string(),
        Value::Text(s) => s,
        _ => String::new(),
    }
}

/// The playlist's current track ids in order.
fn membership_ids(conn: &Connection, playlist_id: &EntityId) -> rusqlite::Result<Vec<String>> {
    conn.prepare(
        "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position, id",
    )?
    .query_map([playlist_id], |r| r.get::<_, Value>(0).map(text_of))?
    .collect()
}

/// Makes `playlist_tracks` hold exactly `track_ids` in order. Rows for tracks that stay keep their id and
/// progress; new rows start at zero. Returns whether the ordered membership changed.
fn materialize(
    conn: &Connection,
    playlist_id: &EntityId,
    track_ids: &[String],
) -> rusqlite::Result<bool> {
    let current = membership_ids(conn, playlist_id)?;
    if current == track_ids {
        return Ok(false);
    }
    let keep: std::collections::HashSet<&str> = track_ids.iter().map(String::as_str).collect();
    let had: std::collections::HashSet<&str> = current.iter().map(String::as_str).collect();
    {
        let mut remove = conn.prepare_cached(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
        )?;
        for id in current.iter().filter(|id| !keep.contains(id.as_str())) {
            remove.execute(rusqlite::params![playlist_id, id])?;
        }
    }
    let mut moved = conn.prepare_cached(
        "UPDATE playlist_tracks SET position = ?3 WHERE playlist_id = ?1 AND track_id = ?2",
    )?;
    let mut added = conn.prepare_cached(
        "INSERT INTO playlist_tracks(id, playlist_id, track_id, position, progress_seconds)
         VALUES (?4, ?1, ?2, ?3, 0)",
    )?;
    for (position, id) in track_ids.iter().enumerate() {
        let position = position as i64;
        if had.contains(id.as_str()) {
            moved.execute(rusqlite::params![playlist_id, id, position])?;
        } else {
            added.execute(rusqlite::params![
                playlist_id,
                id,
                position,
                uuid::Uuid::now_v7().to_string()
            ])?;
        }
    }
    Ok(true)
}

/// Evaluates and stores the membership, recording the revisions it reflects. Returns whether it changed.
/// Reads and writes happen in one savepoint, so the stored result matches one database state.
fn refresh(
    conn: &Connection,
    playlist_id: &EntityId,
    owner: &str,
    def: &ValidDefinition,
) -> Result<bool, SmartError> {
    let now = db_now(conn)?;
    let content = content_revision(conn)?;
    let user = if def.uses_personal() {
        Some(user_revision(conn, owner)?)
    } else {
        None
    };
    in_savepoint(conn, || {
        let selection = evaluate(conn, owner, def, &now)?;
        let changed = materialize(conn, playlist_id, &selection.track_ids)?;
        conn.execute(
            "UPDATE smart_playlist_definitions
             SET evaluated_at = ?1,
                 next_refresh_at = CASE WHEN ?2 THEN datetime(?1, '+60 seconds') END,
                 content_revision = ?3,
                 user_revision = ?4,
                 membership_revision = membership_revision + ?5
             WHERE playlist_id = ?6",
            rusqlite::params![
                now,
                def.has_relative_dates(),
                content,
                user,
                i64::from(changed),
                playlist_id
            ],
        )?;
        Ok(changed)
    })
}

fn is_stale(
    conn: &Connection,
    stored: &Stored,
    def: &ValidDefinition,
    owner: &str,
) -> rusqlite::Result<bool> {
    if stored.evaluated_at.is_none() || stored.content_revision != Some(content_revision(conn)?) {
        return Ok(true);
    }
    if def.uses_personal() && stored.user_revision != Some(user_revision(conn, owner)?) {
        return Ok(true);
    }
    match &stored.next_refresh_at {
        Some(due) => Ok(*due <= db_now(conn)?),
        None => Ok(false),
    }
}

/// Brings a smart playlist's stored tracks up to date when something it depends on changed (or always,
/// with `force`). `Ok(None)` for manual playlists and playlists the user doesn't own; otherwise whether the
/// membership changed. On failure the previous tracks stay.
pub fn ensure_fresh(
    conn: &Connection,
    playlist_id: &EntityId,
    owner: &str,
    force: bool,
) -> Result<Option<bool>, SmartError> {
    let Some(stored) = load_stored(conn, playlist_id, owner)? else {
        return Ok(None);
    };
    let def = stored.parse()?;
    if !force && !is_stale(conn, &stored, &def, owner)? {
        return Ok(Some(false));
    }
    refresh(conn, playlist_id, owner, &def).map(Some)
}

/// For admin paths that act on any user's playlist: refreshes a smart playlist for its owner and returns its
/// membership revision (`Ok(None)` for manual playlists). A refresh that fails keeps the last tracks.
pub fn refresh_for_owner(
    conn: &Connection,
    playlist_id: &EntityId,
) -> rusqlite::Result<Option<i64>> {
    let Some(owner) = smart_owner(conn, playlist_id)? else {
        return Ok(None);
    };
    match ensure_fresh(conn, playlist_id, &owner, false) {
        Err(SmartError::Db(e)) => return Err(e),
        Err(e) => tracing::warn!("Smart playlist refresh failed: {e:?}"),
        Ok(_) => {}
    }
    membership_revision(conn, playlist_id)
}

/// The stored membership revision of a smart playlist (`None` for manual playlists).
pub fn membership_revision(
    conn: &Connection,
    playlist_id: &EntityId,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT membership_revision FROM smart_playlist_definitions WHERE playlist_id = ?1",
        [playlist_id],
        |r| r.get(0),
    )
    .optional()
}

fn smart_owner(conn: &Connection, playlist_id: &EntityId) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT user_id FROM playlists WHERE id = ?1 AND kind = 'smart'",
        [playlist_id],
        |r| r.get::<_, Value>(0).map(text_of),
    )
    .optional()
}

/// Creates a smart playlist, stores its definition and evaluates it, all or nothing.
pub fn create_smart_playlist(
    conn: &Connection,
    name: &str,
    description: &str,
    user_id: &str,
    def: &ValidDefinition,
) -> Result<PlaylistRow, SmartError> {
    in_savepoint(conn, || {
        let row = playlists::create_playlist(conn, name, description, user_id)?;
        conn.execute(
            "UPDATE playlists SET kind = 'smart' WHERE id = ?1",
            [&row.id],
        )?;
        conn.execute(
            "INSERT INTO smart_playlist_definitions(playlist_id, definition_json, schema_version)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![row.id, def.canonical_json(), SCHEMA_VERSION],
        )?;
        refresh(conn, &row.id, user_id, def)?;
        playlists::get_playlist_by_id(conn, &row.id)?.ok_or(SmartError::NotFound)
    })
}

/// A stored definition as the editor loads it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartDefinitionView {
    pub definition: Json,
    pub schema_version: i64,
    pub edit_revision: i64,
    pub membership_revision: i64,
    pub evaluated_at: Option<String>,
    /// False when a newer build stored it; the editor is then read-only.
    pub supported: bool,
    pub missing_library_ids: Vec<String>,
}

/// The owner's smart definition (`None` for manual playlists and other users' playlists).
pub fn smart_definition(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
) -> rusqlite::Result<Option<SmartDefinitionView>> {
    let Some(stored) = load_stored(conn, playlist_id, user_id)? else {
        return Ok(None);
    };
    let parsed = stored.parse().ok();
    let missing_library_ids = match &parsed {
        Some(def) => missing_library_ids(conn, def)?,
        None => Vec::new(),
    };
    Ok(Some(SmartDefinitionView {
        definition: serde_json::from_str(&stored.json).unwrap_or(Json::Null),
        schema_version: stored.schema_version,
        edit_revision: stored.edit_revision,
        membership_revision: stored.membership_revision,
        evaluated_at: stored.evaluated_at,
        supported: parsed.is_some(),
        missing_library_ids,
    }))
}

/// Replaces the rules when `expected_edit_revision` is still current, then re-evaluates, all or nothing.
pub fn update_smart_definition(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
    def: &ValidDefinition,
    expected_edit_revision: i64,
) -> Result<SmartDefinitionView, SmartError> {
    let Some(stored) = load_stored(conn, playlist_id, user_id)? else {
        return Err(SmartError::NotFound);
    };
    if stored.schema_version > i64::from(SCHEMA_VERSION) {
        return Err(SmartError::Unsupported);
    }
    if stored.edit_revision != expected_edit_revision {
        return Err(SmartError::Conflict {
            edit_revision: stored.edit_revision,
        });
    }
    in_savepoint(conn, || {
        conn.execute(
            "UPDATE smart_playlist_definitions
             SET definition_json = ?1, schema_version = ?2, edit_revision = edit_revision + 1,
                 evaluated_at = NULL
             WHERE playlist_id = ?3",
            rusqlite::params![def.canonical_json(), SCHEMA_VERSION, playlist_id],
        )?;
        conn.execute(
            "UPDATE playlists SET updated_at = datetime('now') WHERE id = ?1",
            [playlist_id],
        )?;
        refresh(conn, playlist_id, user_id, def)?;
        Ok::<(), SmartError>(())
    })?;
    smart_definition(conn, playlist_id, user_id)?.ok_or(SmartError::NotFound)
}

/// What unsaved rules would pick. Nothing is stored.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub track_count: usize,
    pub total_duration: f64,
    /// The first [`PREVIEW_SAMPLE`] tracks.
    pub tracks: Vec<PlaylistTrackRow>,
    pub missing_library_ids: Vec<String>,
}

/// Evaluates unsaved rules for `owner`.
pub fn preview(
    conn: &Connection,
    owner: &str,
    def: &ValidDefinition,
) -> Result<Preview, SmartError> {
    let now = db_now(conn)?;
    let selection = evaluate(conn, owner, def, &now)?;
    let sample = &selection.track_ids[..selection.track_ids.len().min(PREVIEW_SAMPLE)];
    Ok(Preview {
        track_count: selection.track_ids.len(),
        total_duration: selection.total_duration,
        tracks: playlists::track_rows_by_ids(conn, sample)?,
        missing_library_ids: missing_library_ids(conn, def)?,
    })
}

/// Saves the current tracks of a smart playlist (refreshed first) as a new ordinary playlist.
pub fn save_as_ordinary(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
    name: &str,
    description: &str,
) -> Result<(PlaylistRow, i64), SmartError> {
    if ensure_fresh(conn, playlist_id, user_id, false)?.is_none() {
        return Err(SmartError::NotFound);
    }
    let ids = membership_ids(conn, playlist_id)?;
    Ok(playlists::create_playlist_with_tracks(
        conn,
        name,
        description,
        user_id,
        &ids,
    )?)
}

/// A playlist with its tracks from one consistent read; smart playlists are refreshed first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSnapshot {
    pub playlist: PlaylistRow,
    pub tracks: Vec<PlaylistTrackRow>,
    /// Smart playlists only; moves whenever the ordered tracks change.
    pub membership_revision: Option<i64>,
    /// Set when a smart playlist could not be refreshed and the last result is shown:
    /// `timed_out`, `unsupported` or `failed`.
    pub refresh_error: Option<&'static str>,
}

/// The user's playlist and tracks (`None` when not theirs). `force` re-evaluates a smart playlist even when
/// nothing it depends on changed.
pub fn playlist_snapshot(
    conn: &Connection,
    playlist_id: &EntityId,
    user_id: &str,
    force: bool,
) -> rusqlite::Result<Option<PlaylistSnapshot>> {
    let refresh_error = match ensure_fresh(conn, playlist_id, user_id, force) {
        Ok(_) => None,
        Err(SmartError::TimedOut) => Some("timed_out"),
        Err(SmartError::Unsupported) => Some("unsupported"),
        Err(e) => {
            tracing::warn!("Smart playlist refresh failed: {e:?}");
            Some("failed")
        }
    };
    let Some(playlist) = playlists::get_playlist(conn, playlist_id, user_id)? else {
        return Ok(None);
    };
    let tracks = playlists::list_playlist_tracks(conn, playlist_id, user_id)?.unwrap_or_default();
    let membership_revision = conn
        .query_row(
            "SELECT membership_revision FROM smart_playlist_definitions WHERE playlist_id = ?1",
            [playlist_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(Some(PlaylistSnapshot {
        playlist,
        tracks,
        membership_revision,
        refresh_error,
    }))
}

// ── Revisions ──────────────────────────────────────────────────────────────────

/// The library-wide revision; moves whenever a track, or a rule/sort input of one, changes.
pub fn content_revision(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT revision FROM smart_playlist_content_revision WHERE id = 1",
        [],
        |r| r.get(0),
    )
    .optional()
    .map(|r| r.unwrap_or(0))
}

/// `user_id`'s personal revision (ratings, plays, track favourites). Starts the counter on first use; triggers
/// only bump existing counters, so deleting a user never recreates one.
pub fn user_revision(conn: &Connection, user_id: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO smart_playlist_user_revisions(user_id, revision)
         SELECT id, 0 FROM users WHERE id = ?1",
        [user_id],
    )?;
    conn.query_row(
        "SELECT revision FROM smart_playlist_user_revisions WHERE user_id = ?1",
        [user_id],
        |r| r.get(0),
    )
    .optional()
    .map(|r| r.unwrap_or(0))
}

// ── Schema (tracked migration `2026-10-29-smart-playlists`) ───────────────────

const BUMP_CONTENT: &str =
    "UPDATE smart_playlist_content_revision SET revision = revision + 1 WHERE id = 1;";

fn bump_user(who: &str) -> String {
    format!(
        "UPDATE smart_playlist_user_revisions SET revision = revision + 1 WHERE user_id = {who}.user_id;"
    )
}

/// Adds smart-playlist storage, revision counters and their triggers, stable `tracks.added_at` (backfilled
/// from `scanned_at`) and the BoogieMix input-snapshot table.
pub(crate) fn ensure_smart_playlists_schema(conn: &Connection) -> rusqlite::Result<()> {
    if crate::table_exists(conn, "playlists") && !crate::column_exists(conn, "playlists", "kind")? {
        conn.execute_batch(
            "ALTER TABLE playlists ADD COLUMN kind TEXT NOT NULL DEFAULT 'manual'
               CHECK(kind IN ('manual','smart'))",
        )?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS smart_playlist_definitions (
           playlist_id          TEXT PRIMARY KEY REFERENCES playlists(id) ON DELETE CASCADE,
           definition_json      TEXT NOT NULL,
           schema_version       INTEGER NOT NULL,
           edit_revision        INTEGER NOT NULL DEFAULT 1,
           membership_revision  INTEGER NOT NULL DEFAULT 0,
           evaluated_at         TEXT,
           next_refresh_at      TEXT,
           content_revision     INTEGER,
           user_revision        INTEGER
         );
         CREATE TABLE IF NOT EXISTS smart_playlist_content_revision (
           id        INTEGER PRIMARY KEY CHECK(id = 1),
           revision  INTEGER NOT NULL DEFAULT 0
         );
         INSERT OR IGNORE INTO smart_playlist_content_revision(id, revision) VALUES (1, 0);
         CREATE TABLE IF NOT EXISTS smart_playlist_user_revisions (
           user_id   TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
           revision  INTEGER NOT NULL DEFAULT 0
         ) WITHOUT ROWID;",
    )?;

    if crate::table_exists(conn, "tracks") {
        if !crate::column_exists(conn, "tracks", "added_at")? {
            conn.execute_batch("ALTER TABLE tracks ADD COLUMN added_at TEXT")?;
        }
        // Existing tracks: the recorded scan date is the best estimate of when they were added.
        let scanned = if crate::column_exists(conn, "tracks", "scanned_at")? {
            "datetime(scanned_at)"
        } else {
            "NULL"
        };
        conn.execute_batch(&format!(
            "UPDATE tracks SET added_at = COALESCE({scanned}, datetime('now'))
               WHERE added_at IS NULL OR TRIM(added_at) = '';
             CREATE TRIGGER IF NOT EXISTS trg_tracks_added_at AFTER INSERT ON tracks
               WHEN NEW.added_at IS NULL
             BEGIN
               UPDATE tracks SET added_at = datetime('now') WHERE rowid = NEW.rowid;
             END;"
        ))?;
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_content_track_insert AFTER INSERT ON tracks
             BEGIN {BUMP_CONTENT} END;
             CREATE TRIGGER IF NOT EXISTS trg_smart_content_track_delete AFTER DELETE ON tracks
             BEGIN {BUMP_CONTENT} END;"
        ))?;
    }
    let rule_columns = [
        "title",
        "genre",
        "year",
        "library_id",
        "artist_id",
        "album_id",
        "album_artist",
        "duration",
        "bpm",
        "bpm_detected",
        "added_at",
    ];
    let mut has_rule_columns = crate::table_exists(conn, "tracks");
    for column in rule_columns {
        has_rule_columns = has_rule_columns && crate::column_exists(conn, "tracks", column)?;
    }
    // Minimal legacy `tracks` tables (old fixtures) lack some columns; real databases have them all.
    if has_rule_columns {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_content_track_update
               AFTER UPDATE OF title, genre, year, library_id, artist_id, album_id, album_artist, duration,
                               bpm, bpm_detected, added_at ON tracks
               WHEN NEW.title IS NOT OLD.title OR NEW.genre IS NOT OLD.genre OR NEW.year IS NOT OLD.year
                 OR NEW.library_id IS NOT OLD.library_id OR NEW.artist_id IS NOT OLD.artist_id
                 OR NEW.album_id IS NOT OLD.album_id OR NEW.album_artist IS NOT OLD.album_artist
                 OR NEW.duration IS NOT OLD.duration OR NEW.bpm IS NOT OLD.bpm
                 OR NEW.bpm_detected IS NOT OLD.bpm_detected OR NEW.added_at IS NOT OLD.added_at
             BEGIN {BUMP_CONTENT} END;"
        ))?;
    }
    if crate::table_exists(conn, "artists") {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_content_artist_name AFTER UPDATE OF name ON artists
               WHEN NEW.name IS NOT OLD.name
             BEGIN {BUMP_CONTENT} END;"
        ))?;
    }
    if crate::table_exists(conn, "albums") {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_content_album_title AFTER UPDATE OF title ON albums
               WHEN NEW.title IS NOT OLD.title
             BEGIN {BUMP_CONTENT} END;"
        ))?;
    }

    let (new, old) = (bump_user("NEW"), bump_user("OLD"));
    if crate::table_exists(conn, "track_ratings") {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_user_rating_insert AFTER INSERT ON track_ratings
             BEGIN {new} END;
             CREATE TRIGGER IF NOT EXISTS trg_smart_user_rating_update AFTER UPDATE OF rating ON track_ratings
               WHEN NEW.rating IS NOT OLD.rating
             BEGIN {new} END;
             CREATE TRIGGER IF NOT EXISTS trg_smart_user_rating_delete AFTER DELETE ON track_ratings
             BEGIN {old} END;"
        ))?;
    }
    if crate::table_exists(conn, "play_history") {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_user_play_insert AFTER INSERT ON play_history
             BEGIN {new} END;
             CREATE TRIGGER IF NOT EXISTS trg_smart_user_play_delete AFTER DELETE ON play_history
             BEGIN {old} END;"
        ))?;
    }
    if crate::table_exists(conn, "subsonic_stars") {
        conn.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS trg_smart_user_star_insert AFTER INSERT ON subsonic_stars
               WHEN NEW.item_type = 'track'
             BEGIN {new} END;
             CREATE TRIGGER IF NOT EXISTS trg_smart_user_star_delete AFTER DELETE ON subsonic_stars
               WHEN OLD.item_type = 'track'
             BEGIN {old} END;"
        ))?;
    }

    if crate::table_exists(conn, "mix_jobs") {
        for (column, definition) in [
            ("input_snapshot", "INTEGER NOT NULL DEFAULT 0"),
            ("source_definition_revision", "INTEGER"),
            ("source_membership_revision", "INTEGER"),
        ] {
            if !crate::column_exists(conn, "mix_jobs", column)? {
                conn.execute_batch(&format!(
                    "ALTER TABLE mix_jobs ADD COLUMN {column} {definition}"
                ))?;
            }
        }
        // Input rows keep their own copy of each track (no FK to tracks): a removed source track must not
        // silently change an accepted mix.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS mix_job_tracks (
               job_id        TEXT NOT NULL REFERENCES mix_jobs(id) ON DELETE CASCADE,
               position      INTEGER NOT NULL,
               track_id      TEXT NOT NULL,
               file_path     TEXT NOT NULL,
               title         TEXT,
               artist        TEXT,
               duration      REAL,
               bpm           REAL,
               bpm_detected  REAL,
               file_size     INTEGER,
               scanned_at    TEXT,
               PRIMARY KEY (job_id, position)
             ) WITHOUT ROWID;",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: &str = "2026-11-01 12:00:00";

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn
    }

    /// The §2a.1 fixture: owner `alice`, plus `bob` for per-user checks.
    fn fixture() -> Connection {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES ('alice', 'alice'), ('bob', 'bob');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'Music'), ('lib2', '/m2', 'Vinyl');
             INSERT INTO artists(id, name) VALUES ('a1', 'Massive Attack'), ('a2', 'Bonobo'), ('a3', 'Air');
             INSERT INTO albums(id, title) VALUES
               ('al1', 'Mezzanine'), ('al2', 'Migration'), ('al3', 'Dummy'), ('al4', '100%_Pure');
             INSERT INTO tracks(id, library_id, artist_id, album_id, album_artist, title, file_path, genre, year,
                                duration, added_at, bpm, bpm_detected) VALUES
               ('t1', 'lib1', 'a1', 'al1', '', 'Arc', '/m1/1', 'Electronic', 1998, 300, '2026-10-20 00:00:00', 0, 122.4),
               ('t2', 'lib1', 'a2', 'al2', 'Various Artists', 'Bloom', '/m1/2', 'Electronic, Ambient', 2004, 420,
                      '2026-06-01 00:00:00', 96, NULL),
               ('t3', 'lib2', NULL, 'al3', 'Portishead', 'Cove', '/m2/3', 'Rock', 1998, 200, '2026-10-31 00:00:00', NULL, NULL),
               ('t4', 'lib2', 'a2', 'al4', '', 'Dusk', '/m2/4', 'Ambient', NULL, 600, '2025-12-01 00:00:00', 70, 140),
               ('t5', 'lib1', 'a3', NULL, '', 'Echo', '/m1/5', '', 2010, 0, '2026-10-29 00:00:00', NULL, NULL);
             INSERT INTO track_ratings(user_id, track_id, rating) VALUES
               ('alice', 't1', 4.5), ('alice', 't2', 4), ('alice', 't4', 5), ('alice', 't5', 3), ('bob', 't3', 5);
             INSERT INTO play_history(id, user_id, track_id, played_at) VALUES
               ('h1', 'alice', 't1', '2026-10-28 10:00:00'), ('h2', 'alice', 't1', '2026-10-29 10:00:00'),
               ('h3', 'alice', 't1', '2026-10-30 10:00:00'), ('h4', 'alice', 't3', '2026-05-01 10:00:00');
             INSERT INTO subsonic_stars(user_id, item_type, item_id) VALUES
               ('alice', 'track', 't3'), ('alice', 'album', 'al1');",
        )
        .unwrap();
        for i in 0..10 {
            conn.execute(
                "INSERT INTO play_history(id, user_id, track_id, played_at) VALUES (?1, 'alice', 't4', ?2)",
                rusqlite::params![format!("d{i}"), format!("2026-07-{:02} 10:00:00", 6 + i)],
            )
            .unwrap();
        }
        conn
    }

    fn rule(field: &str, operator: &str, value: Option<Json>) -> Json {
        let mut r = json!({ "field": field, "operator": operator });
        if let Some(v) = value {
            r["value"] = v;
        }
        r
    }

    fn def(mode: &str, children: Vec<Json>) -> Json {
        json!({ "schemaVersion": 1, "root": { "mode": mode, "children": children } })
    }

    fn titles(conn: &Connection, owner: &str, definition: Json) -> Vec<String> {
        let valid = parse_definition(&definition.to_string()).expect("valid definition");
        let selection = evaluate(conn, owner, &valid, NOW).unwrap();
        selection
            .track_ids
            .iter()
            .map(|id| {
                conn.query_row("SELECT title FROM tracks WHERE id = ?1", [id], |r| r.get(0))
                    .unwrap()
            })
            .collect()
    }

    fn codes(definition: Json) -> Vec<RuleErrorCode> {
        parse_definition(&definition.to_string())
            .expect_err("invalid definition")
            .into_iter()
            .map(|e| e.code)
            .collect()
    }

    #[test]
    fn worked_examples_from_the_plan() {
        let conn = fixture();
        let cases: Vec<(&str, Json, Vec<&str>)> = vec![
            (
                "E1",
                def(
                    "all",
                    vec![
                        rule("genre", "is", Some(json!("Electronic"))),
                        rule("rating", "gte", Some(json!(4))),
                    ],
                ),
                vec!["Arc", "Bloom"],
            ),
            (
                "E2",
                def(
                    "all",
                    vec![rule("last_played_at", "not_in_last_days", Some(json!(90)))],
                ),
                vec!["Bloom", "Cove", "Dusk", "Echo"],
            ),
            (
                "E3",
                def(
                    "any",
                    vec![
                        rule("genre", "is_missing", None),
                        rule("year", "is_missing", None),
                    ],
                ),
                vec!["Dusk", "Echo"],
            ),
            (
                "E4",
                def("all", vec![rule("bpm", "gte", Some(json!(120)))]),
                vec!["Arc"],
            ),
            (
                "E5",
                def("all", vec![rule("genre", "is_not", Some(json!("Rock")))]),
                vec!["Arc", "Bloom", "Dusk"],
            ),
            (
                "E6",
                def("all", vec![rule("rating", "lte", Some(json!(4)))]),
                vec!["Bloom", "Echo"],
            ),
            (
                "E9",
                def(
                    "all",
                    vec![rule("added_at", "in_last_days", Some(json!(7)))],
                ),
                vec!["Cove", "Echo"],
            ),
            (
                "E10",
                def("all", vec![rule("play_count", "is", Some(json!(0)))]),
                vec!["Bloom", "Echo"],
            ),
            (
                "E11",
                def("all", vec![rule("favourite", "is", None)]),
                vec!["Cove"],
            ),
        ];
        for (name, definition, expected) in cases {
            assert_eq!(titles(&conn, "alice", definition), expected, "{name}");
        }

        let mut e7 = def("all", vec![rule("genre", "is_present", None)]);
        e7["limit"] = json!({ "maxDurationSeconds": 900 });
        assert_eq!(
            titles(&conn, "alice", e7),
            ["Arc", "Bloom"],
            "E7: stop, don't skip"
        );

        let mut e8 = def("all", vec![rule("genre", "is_present", None)]);
        e8["sort"] = json!([{ "field": "last_played_at", "direction": "asc" }]);
        e8["limit"] = json!({ "maxTracks": 2 });
        assert_eq!(
            titles(&conn, "alice", e8),
            ["Cove", "Dusk"],
            "E8: never played sorts last"
        );
    }

    #[test]
    fn artist_and_album_rules() {
        let conn = fixture();
        let one = |field: &str, op: &str, value: &str| {
            titles(
                &conn,
                "alice",
                def("all", vec![rule(field, op, Some(json!(value)))]),
            )
        };
        assert_eq!(
            one("artist", "is", "Portishead"),
            ["Cove"],
            "album artist counts"
        );
        assert_eq!(
            one("artist", "is", "various artists"),
            ["Bloom"],
            "case-insensitive"
        );
        assert_eq!(one("artist", "contains", "bon"), ["Bloom", "Dusk"]);
        assert_eq!(one("artist", "is_not", "Bonobo"), ["Arc", "Cove", "Echo"]);
        assert_eq!(one("artist", "not_contains", "o"), ["Arc", "Echo"]);
        assert_eq!(
            one("album", "is_not", "Mezzanine"),
            ["Bloom", "Cove", "Dusk"],
            "no album is excluded"
        );
        assert_eq!(
            one("album", "contains", "%_"),
            ["Dusk"],
            "no LIKE wildcards"
        );
        assert_eq!(one("album", "contains", "_"), ["Dusk"]);
        assert!(
            one("artist", "is", "x' OR 1=1 --").is_empty(),
            "values are bound"
        );
    }

    #[test]
    fn library_year_bpm_dates_and_presence() {
        let conn = fixture();
        let all = |children: Vec<Json>| titles(&conn, "alice", def("all", children));
        assert_eq!(
            all(vec![rule(
                "library_id",
                "in",
                Some(json!(["lib2", "gone"]))
            )]),
            ["Cove", "Dusk"],
            "a removed library matches nothing"
        );
        assert_eq!(
            all(vec![rule("library_id", "not_in", Some(json!(["lib2"])))]),
            ["Arc", "Bloom", "Echo"]
        );
        assert_eq!(
            all(vec![rule("year", "between", Some(json!([1998, 2004])))]),
            ["Arc", "Bloom", "Cove"]
        );
        assert_eq!(
            all(vec![rule("year", "lte", Some(json!(2000)))]),
            ["Arc", "Cove"],
            "unknown year excluded"
        );
        assert_eq!(
            all(vec![rule("bpm", "is", Some(json!(122)))]),
            ["Arc"],
            "BPM is compares rounded"
        );
        assert_eq!(all(vec![rule("bpm", "is_missing", None)]), ["Cove", "Echo"]);
        assert_eq!(all(vec![rule("rating", "is_missing", None)]), ["Cove"]);
        assert_eq!(
            all(vec![rule("rating", "between", Some(json!([4, 4.5])))]),
            ["Arc", "Bloom"]
        );
        assert_eq!(
            all(vec![rule("added_at", "before", Some(json!("2026-06-01")))]),
            ["Dusk"]
        );
        assert_eq!(
            all(vec![rule(
                "added_at",
                "on_or_after",
                Some(json!("2026-06-01"))
            )]),
            ["Arc", "Bloom", "Cove", "Echo"],
            "on or after is inclusive"
        );
        assert_eq!(
            all(vec![rule("added_at", "not_in_last_days", Some(json!(30)))]),
            ["Bloom", "Dusk"]
        );
        assert_eq!(
            all(vec![rule("last_played_at", "is_missing", None)]),
            ["Bloom", "Echo"]
        );
        assert_eq!(
            all(vec![rule("last_played_at", "is_present", None)]),
            ["Arc", "Cove", "Dusk"]
        );
        assert_eq!(
            all(vec![rule("last_played_at", "in_last_days", Some(json!(7)))]),
            ["Arc"]
        );
        assert_eq!(
            all(vec![rule("play_count", "gte", Some(json!(3)))]),
            ["Arc", "Dusk"]
        );
        assert_eq!(
            all(vec![rule("favourite", "is_not", None)]),
            ["Arc", "Bloom", "Dusk", "Echo"]
        );
    }

    #[test]
    fn nested_groups_and_sorting() {
        let conn = fixture();
        let mut d = def(
            "all",
            vec![
                rule("library_id", "in", Some(json!(["lib1", "lib2"]))),
                json!({ "mode": "any", "children": [
                    rule("genre", "is", Some(json!("Ambient"))),
                    rule("year", "is", Some(json!(1998))),
                ]}),
            ],
        );
        d["sort"] = json!([
            { "field": "year", "direction": "desc" },
            { "field": "title", "direction": "desc" }
        ]);
        // Bloom 2004, Cove 1998, Arc 1998, Dusk (no year) last.
        assert_eq!(titles(&conn, "alice", d), ["Bloom", "Cove", "Arc", "Dusk"]);

        let mut by_rating = def("all", vec![rule("genre", "is_present", None)]);
        by_rating["sort"] = json!([{ "field": "rating", "direction": "desc" }]);
        assert_eq!(
            titles(&conn, "alice", by_rating),
            ["Dusk", "Arc", "Bloom", "Cove"],
            "unrated last"
        );

        let mut by_artist = def("all", vec![rule("genre", "is_present", None)]);
        by_artist["sort"] = json!([{ "field": "artist", "direction": "asc" }]);
        assert_eq!(
            titles(&conn, "alice", by_artist),
            ["Bloom", "Dusk", "Arc", "Cove"]
        );
    }

    #[test]
    fn personal_rules_differ_per_user() {
        let conn = fixture();
        let rated = def("all", vec![rule("rating", "gte", Some(json!(4)))]);
        assert_eq!(
            titles(&conn, "alice", rated.clone()),
            ["Arc", "Bloom", "Dusk"]
        );
        assert_eq!(titles(&conn, "bob", rated), ["Cove"]);
        let never = def("all", vec![rule("last_played_at", "is_missing", None)]);
        assert_eq!(titles(&conn, "bob", never).len(), 5);
        let fav = def("all", vec![rule("favourite", "is", None)]);
        assert!(titles(&conn, "bob", fav).is_empty());
    }

    #[test]
    fn canonical_form_trims_and_dedupes() {
        let valid = parse_definition(
            &def(
                "all",
                vec![
                    rule("artist", "is", Some(json!("  Air  "))),
                    rule("library_id", "in", Some(json!(["lib1", " lib1 ", 7]))),
                ],
            )
            .to_string(),
        )
        .unwrap();
        let canonical: Json = serde_json::from_str(&valid.canonical_json()).unwrap();
        assert_eq!(canonical["root"]["children"][0]["value"], json!("Air"));
        assert_eq!(
            canonical["root"]["children"][1]["value"],
            json!(["lib1", "7"])
        );
        assert_eq!(canonical["limit"]["maxTracks"], json!(100));
        assert!(!valid.has_relative_dates());
        let timed = parse_definition(
            &def("all", vec![json!({ "mode": "any", "children": [rule("added_at", "in_last_days", Some(json!(3)))] })])
                .to_string(),
        )
        .unwrap();
        assert!(timed.has_relative_dates());
    }

    #[test]
    fn validation_rejects_bad_definitions() {
        use RuleErrorCode::*;
        assert_eq!(
            codes(json!({ "schemaVersion": 2, "root": { "mode": "all", "children": [] } })),
            [UnsupportedVersion]
        );
        assert_eq!(
            codes(
                json!({ "schemaVersion": 1, "root": { "mode": "all", "children": [] }, "extra": 1 })
            ),
            [Invalid]
        );
        assert_eq!(codes(def("all", vec![])), [EmptyGroup]);
        assert_eq!(
            codes(def("all", vec![rule("path", "is", Some(json!("x")))])),
            [UnknownField]
        );
        assert_eq!(
            codes(def("all", vec![rule("genre", "gte", Some(json!(1)))])),
            [UnknownOperator]
        );
        assert_eq!(
            codes(def("all", vec![rule("genre", "is", None)])),
            [MissingValue]
        );
        assert_eq!(
            codes(def(
                "all",
                vec![rule("genre", "is_missing", Some(json!("x")))]
            )),
            [UnexpectedValue]
        );
        assert_eq!(
            codes(def("all", vec![rule("favourite", "is", Some(json!(true)))])),
            [UnexpectedValue]
        );
        assert_eq!(
            codes(def("all", vec![rule("artist", "is", Some(json!("   ")))])),
            [BadValue]
        );
        assert_eq!(
            codes(def(
                "all",
                vec![rule("artist", "is", Some(json!("x".repeat(129))))]
            )),
            [BadValue]
        );
        for (field, value) in [
            ("year", json!(0)),
            ("year", json!(10_000)),
            ("year", json!(1998.5)),
            ("rating", json!(0)),
            ("rating", json!(4.25)),
            ("rating", json!(5.5)),
            ("bpm", json!(0)),
            ("bpm", json!(1000.5)),
            ("play_count", json!(-1)),
            ("play_count", json!(2_147_483_648_i64)),
            ("year", json!("1998")),
        ] {
            assert_eq!(
                codes(def("all", vec![rule(field, "is", Some(value.clone()))])),
                [BadValue],
                "{field} {value}"
            );
        }
        assert_eq!(
            codes(def(
                "all",
                vec![rule("year", "between", Some(json!([2001, 1994])))]
            )),
            [BadRange]
        );
        assert_eq!(
            codes(def(
                "all",
                vec![rule("year", "between", Some(json!([1994])))]
            )),
            [BadValue]
        );
        assert_eq!(
            codes(def(
                "all",
                vec![rule("added_at", "in_last_days", Some(json!(0)))]
            )),
            [BadValue]
        );
        assert_eq!(
            codes(def(
                "all",
                vec![rule("added_at", "in_last_days", Some(json!(3651)))]
            )),
            [BadValue]
        );
        for date in [
            "2026-02-29",
            "2026-13-01",
            "2026-1-01",
            "26-01-01",
            "2026-01-01T00:00:00",
        ] {
            assert_eq!(
                codes(def(
                    "all",
                    vec![rule("added_at", "before", Some(json!(date)))]
                )),
                [BadValue],
                "{date}"
            );
        }
        assert!(parse_definition(
            &def(
                "all",
                vec![rule("added_at", "before", Some(json!("2024-02-29")))]
            )
            .to_string()
        )
        .is_ok());
        assert_eq!(
            codes(def("all", vec![rule("library_id", "in", Some(json!([])))])),
            [BadValue]
        );
        let many: Vec<String> = (0..101).map(|i| format!("l{i}")).collect();
        assert_eq!(
            codes(def(
                "all",
                vec![rule("library_id", "in", Some(json!(many)))]
            )),
            [BadValue]
        );

        let mut sorted = def("all", vec![rule("genre", "is_present", None)]);
        sorted["sort"] = json!([{ "field": "path", "direction": "asc" }]);
        assert_eq!(codes(sorted.clone()), [BadSort]);
        sorted["sort"] = json!([
            { "field": "title", "direction": "asc" },
            { "field": "title", "direction": "desc" }
        ]);
        assert_eq!(codes(sorted.clone()), [BadSort]);
        sorted["sort"] = json!([
            { "field": "title", "direction": "asc" },
            { "field": "year", "direction": "asc" },
            { "field": "bpm", "direction": "asc" }
        ]);
        assert_eq!(codes(sorted), [BadSort]);
        for limit in [
            json!({ "maxTracks": 0 }),
            json!({ "maxTracks": 1001 }),
            json!({ "maxDurationSeconds": 86_401 }),
        ] {
            let mut limited = def("all", vec![rule("genre", "is_present", None)]);
            limited["limit"] = limit;
            assert_eq!(codes(limited), [BadLimit]);
        }
        let mut negative = def("all", vec![rule("genre", "is_present", None)]);
        negative["limit"] = json!({ "maxTracks": -5 });
        assert_eq!(codes(negative), [Invalid]);
        assert_eq!(parse_definition("{ not json").unwrap_err()[0].code, Invalid);
        assert_eq!(
            parse_definition(&" ".repeat(MAX_DEFINITION_BYTES + 1)).unwrap_err()[0].code,
            TooLarge
        );
    }

    #[test]
    fn validation_bounds_depth_and_counts() {
        use RuleErrorCode::*;
        let leaf = || rule("genre", "is_present", None);
        let depth3 = def(
            "all",
            vec![json!({ "mode": "any", "children": [ { "mode": "all", "children": [leaf()] } ] })],
        );
        assert!(
            parse_definition(&depth3.to_string()).is_ok(),
            "root + two levels"
        );
        let depth4 = def(
            "all",
            vec![
                json!({ "mode": "any", "children": [ { "mode": "all", "children": [ { "mode": "any", "children": [leaf()] } ] } ] }),
            ],
        );
        assert_eq!(codes(depth4), [TooDeep]);
        assert!(
            parse_definition(&def("all", (0..24).map(|_| leaf()).collect()).to_string()).is_ok()
        );
        assert_eq!(
            codes(def("all", (0..25).map(|_| leaf()).collect())),
            [TooManyRules]
        );
        // 1 root + 9 groups + 23 rules = 33 nodes.
        let mut children: Vec<Json> = (0..9)
            .map(|_| json!({ "mode": "all", "children": [leaf()] }))
            .collect();
        children.extend((0..14).map(|_| leaf()));
        assert_eq!(codes(def("all", children)), [TooManyNodes]);
        let errors = parse_definition(
            &def(
                "all",
                vec![rule("path", "is", None), rule("genre", "is", None)],
            )
            .to_string(),
        )
        .unwrap_err();
        assert_eq!(
            errors,
            [
                RuleError {
                    path: "root.children[0].field".into(),
                    code: UnknownField
                },
                RuleError {
                    path: "root.children[1].value".into(),
                    code: MissingValue
                },
            ]
        );
    }

    #[test]
    fn query_budget_interrupts_and_the_connection_recovers() {
        let conn = db();
        let slow = with_query_budget(&conn, Duration::from_millis(20), || {
            conn.query_row(
                "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT MAX(x) FROM c",
                [],
                |r| r.get::<_, i64>(0),
            )
        });
        assert!(matches!(slow, Err(EvalError::TimedOut)));
        let fine: i64 = conn.query_row("SELECT 41 + 1", [], |r| r.get(0)).unwrap();
        assert_eq!(fine, 42);
        assert_eq!(
            with_query_budget(&conn, Duration::from_secs(5), || Ok(7)).unwrap(),
            7
        );
    }

    #[test]
    fn migration_backfills_added_at_and_is_idempotent() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'Music');
             INSERT INTO tracks(id, library_id, title, file_path, scanned_at) VALUES
               ('t1', 'lib1', 'A', '/m1/a', '2024-03-04 05:06:07'),
               ('t2', 'lib1', 'B', '/m1/b', 'garbage');
             UPDATE tracks SET added_at = NULL;",
        )
        .unwrap();
        ensure_smart_playlists_schema(&conn).unwrap();
        ensure_smart_playlists_schema(&conn).unwrap();
        let added: Vec<String> = conn
            .prepare("SELECT added_at FROM tracks ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(added[0], "2024-03-04 05:06:07");
        assert_eq!(
            added[1].len(),
            19,
            "invalid scan date falls back to the migration time"
        );

        let kind: String = {
            conn.execute("INSERT INTO playlists(id, name) VALUES ('p1', 'P')", [])
                .unwrap();
            conn.query_row("SELECT kind FROM playlists WHERE id = 'p1'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(kind, "manual");
        assert!(conn
            .execute("UPDATE playlists SET kind = 'other' WHERE id = 'p1'", [])
            .is_err());
        assert!(crate::column_exists(&conn, "mix_jobs", "input_snapshot").unwrap());
        assert!(crate::table_exists(&conn, "mix_job_tracks"));
    }

    #[test]
    fn inserted_tracks_get_an_added_date() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'Music');
             INSERT INTO tracks(id, library_id, title, file_path) VALUES ('t1', 'lib1', 'A', '/m1/a');
             INSERT INTO tracks(id, library_id, title, file_path, added_at)
               VALUES ('t2', 'lib1', 'B', '/m1/b', '2020-01-01 00:00:00');",
        )
        .unwrap();
        let get = |id: &str| -> Option<String> {
            conn.query_row("SELECT added_at FROM tracks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(get("t1").map(|s| s.len()), Some(19));
        assert_eq!(get("t2").as_deref(), Some("2020-01-01 00:00:00"));
    }

    #[test]
    fn revision_triggers() {
        let conn = fixture();
        let content = || content_revision(&conn).unwrap();
        let before = content();
        conn.execute("UPDATE tracks SET title = 'Arc' WHERE id = 't1'", [])
            .unwrap();
        conn.execute(
            "UPDATE tracks SET play_count = 9, file_size = 5 WHERE id = 't1'",
            [],
        )
        .unwrap();
        assert_eq!(
            content(),
            before,
            "unchanged or untracked columns don't count"
        );
        conn.execute("UPDATE tracks SET title = 'Arc II' WHERE id = 't1'", [])
            .unwrap();
        assert!(content() > before);
        let after_title = content();
        conn.execute("UPDATE artists SET name = 'Massive' WHERE id = 'a1'", [])
            .unwrap();
        assert!(content() > after_title);
        let after_artist = content();
        conn.execute("DELETE FROM tracks WHERE id = 't5'", [])
            .unwrap();
        assert!(content() > after_artist);

        assert_eq!(user_revision(&conn, "alice").unwrap(), 0);
        assert_eq!(
            user_revision(&conn, "nobody").unwrap(),
            0,
            "unknown users get no counter"
        );
        conn.execute(
            "UPDATE track_ratings SET rating = 2 WHERE user_id = 'alice' AND track_id = 't1'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO play_history(id, user_id, track_id) VALUES ('x1', 'alice', 't2')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO subsonic_stars(user_id, item_type, item_id) VALUES ('alice', 'track', 't2')", []).unwrap();
        conn.execute("INSERT INTO subsonic_stars(user_id, item_type, item_id) VALUES ('alice', 'album', 'al2')", []).unwrap();
        assert_eq!(
            user_revision(&conn, "alice").unwrap(),
            3,
            "album stars don't count"
        );
        conn.execute(
            "INSERT INTO play_history(id, user_id, track_id) VALUES ('x2', 'bob', 't2')",
            [],
        )
        .unwrap();
        assert_eq!(
            user_revision(&conn, "bob").unwrap(),
            0,
            "bob's counter starts on first use"
        );
        assert_eq!(user_revision(&conn, "alice").unwrap(), 3);

        conn.execute("DELETE FROM users WHERE id = 'alice'", [])
            .unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM smart_playlist_user_revisions WHERE user_id = 'alice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0, "deleting a user leaves no counter behind");
    }

    fn valid(definition: Json) -> ValidDefinition {
        parse_definition(&definition.to_string()).unwrap()
    }

    fn member_titles(conn: &Connection, playlist: &EntityId) -> Vec<String> {
        conn.prepare(
            "SELECT t.title FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
             WHERE pt.playlist_id = ?1 ORDER BY pt.position",
        )
        .unwrap()
        .query_map([playlist], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn stored_playlists_refresh_when_inputs_change() {
        let conn = fixture();
        let rated = valid(def("all", vec![rule("rating", "gte", Some(json!(4)))]));
        let row = create_smart_playlist(&conn, "Top", "", "alice", &rated).unwrap();
        assert_eq!(row.kind, "smart");
        assert_eq!(row.track_count, 3);
        assert_eq!(member_titles(&conn, &row.id), ["Arc", "Bloom", "Dusk"]);
        let updated_at = row.updated_at.clone();

        // Keep progress on a retained row so we can see rows survive refreshes.
        conn.execute(
            "UPDATE playlist_tracks SET progress_seconds = 42 WHERE playlist_id = ?1 AND track_id = 't2'",
            [&row.id],
        )
        .unwrap();
        let bloom_row: String = conn
            .query_row(
                "SELECT id FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = 't2'",
                [&row.id],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(
            ensure_fresh(&conn, &row.id, "alice", false).unwrap(),
            Some(false),
            "nothing changed"
        );
        conn.execute(
            "INSERT INTO track_ratings(user_id, track_id, rating) VALUES ('alice', 't3', 5)",
            [],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM track_ratings WHERE user_id = 'alice' AND track_id = 't1'",
            [],
        )
        .unwrap();
        assert_eq!(
            ensure_fresh(&conn, &row.id, "alice", false).unwrap(),
            Some(true)
        );
        assert_eq!(member_titles(&conn, &row.id), ["Bloom", "Cove", "Dusk"]);
        let (kept_id, progress): (String, f64) = conn
            .query_row(
                "SELECT id, progress_seconds FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = 't2'",
                [&row.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (kept_id, progress),
            (bloom_row, 42.0),
            "retained rows keep id and progress"
        );

        // Bob's activity doesn't touch Alice's playlist; library edits do.
        conn.execute(
            "INSERT INTO track_ratings(user_id, track_id, rating) VALUES ('bob', 't1', 5)",
            [],
        )
        .unwrap();
        assert_eq!(
            ensure_fresh(&conn, &row.id, "alice", false).unwrap(),
            Some(false)
        );
        conn.execute(
            "UPDATE tracks SET title = 'Bloom (Edit)' WHERE id = 't2'",
            [],
        )
        .unwrap();
        assert_eq!(
            ensure_fresh(&conn, &row.id, "alice", false).unwrap(),
            Some(false),
            "re-evaluated, same tracks"
        );
        let updated_now: Option<String> = conn
            .query_row(
                "SELECT updated_at FROM playlists WHERE id = ?1",
                [&row.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            updated_now, updated_at,
            "refreshes don't reorder playlist cards"
        );

        assert_eq!(
            ensure_fresh(&conn, &row.id, "bob", false).unwrap(),
            None,
            "not bob's playlist"
        );
        let manual = playlists::create_playlist(&conn, "Plain", "", "alice").unwrap();
        assert_eq!(
            ensure_fresh(&conn, &manual.id, "alice", false).unwrap(),
            None
        );
    }

    #[test]
    fn non_personal_rules_ignore_plays_and_time_rules_expire() {
        let conn = fixture();
        let genre = valid(def(
            "all",
            vec![rule("genre", "is", Some(json!("Ambient")))],
        ));
        let row = create_smart_playlist(&conn, "Ambient", "", "alice", &genre).unwrap();
        conn.execute(
            "INSERT INTO play_history(id, user_id, track_id) VALUES ('z1', 'alice', 't2')",
            [],
        )
        .unwrap();
        let before: Option<String> = conn
            .query_row(
                "SELECT evaluated_at FROM smart_playlist_definitions WHERE playlist_id = ?1",
                [&row.id],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute("UPDATE smart_playlist_definitions SET evaluated_at = '2000-01-01 00:00:00' WHERE playlist_id = ?1", [&row.id]).unwrap();
        assert_eq!(
            ensure_fresh(&conn, &row.id, "alice", false).unwrap(),
            Some(false)
        );
        let after: Option<String> = conn
            .query_row(
                "SELECT evaluated_at FROM smart_playlist_definitions WHERE playlist_id = ?1",
                [&row.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            after.as_deref(),
            Some("2000-01-01 00:00:00"),
            "a play doesn't re-evaluate a genre rule"
        );
        assert!(before.is_some());

        let recent = valid(def(
            "all",
            vec![rule("added_at", "in_last_days", Some(json!(30)))],
        ));
        let timed = create_smart_playlist(&conn, "Recent", "", "alice", &recent).unwrap();
        let due: Option<String> = conn
            .query_row(
                "SELECT next_refresh_at FROM smart_playlist_definitions WHERE playlist_id = ?1",
                [&timed.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(due.is_some());
        conn.execute("UPDATE smart_playlist_definitions SET next_refresh_at = '2000-01-01 00:00:00' WHERE playlist_id = ?1", [&timed.id]).unwrap();
        ensure_fresh(&conn, &timed.id, "alice", false).unwrap();
        let due_again: String = conn
            .query_row(
                "SELECT next_refresh_at FROM smart_playlist_definitions WHERE playlist_id = ?1",
                [&timed.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            due_again.as_str() > "2000-01-01 00:00:00",
            "an expired result is re-evaluated"
        );
    }

    #[test]
    fn editing_rules_checks_the_revision_and_is_atomic() {
        let conn = fixture();
        let row = create_smart_playlist(
            &conn,
            "Edit me",
            "",
            "alice",
            &valid(def("all", vec![rule("genre", "is", Some(json!("Rock")))])),
        )
        .unwrap();
        let view = smart_definition(&conn, &row.id, "alice").unwrap().unwrap();
        assert_eq!(
            (view.edit_revision, view.membership_revision, view.supported),
            (1, 1, true)
        );
        let rock = member_titles(&conn, &row.id);
        assert_eq!(rock, ["Cove"]);

        let ambient = valid(def(
            "all",
            vec![rule("genre", "is", Some(json!("Ambient")))],
        ));
        let saved = update_smart_definition(&conn, &row.id, "alice", &ambient, 1).unwrap();
        assert_eq!(saved.edit_revision, 2);
        assert_eq!(member_titles(&conn, &row.id), ["Bloom", "Dusk"]);
        assert!(matches!(
            update_smart_definition(&conn, &row.id, "alice", &ambient, 1),
            Err(SmartError::Conflict { edit_revision: 2 })
        ));
        assert!(matches!(
            update_smart_definition(&conn, &row.id, "bob", &ambient, 2),
            Err(SmartError::NotFound)
        ));

        // A newer build's definition is kept and never overwritten.
        conn.execute(
            "UPDATE smart_playlist_definitions SET schema_version = 2 WHERE playlist_id = ?1",
            [&row.id],
        )
        .unwrap();
        let view = smart_definition(&conn, &row.id, "alice").unwrap().unwrap();
        assert!(!view.supported);
        assert!(matches!(
            update_smart_definition(&conn, &row.id, "alice", &ambient, 2),
            Err(SmartError::Unsupported)
        ));
        let snapshot = playlist_snapshot(&conn, &row.id, "alice", true)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.refresh_error, Some("unsupported"));
        assert_eq!(snapshot.tracks.len(), 2, "the last result stays");
    }

    #[test]
    fn create_rolls_back_and_names_are_shared() {
        let conn = fixture();
        playlists::create_playlist(&conn, "Taken", "", "alice").unwrap();
        let rules = valid(def("all", vec![rule("genre", "is_present", None)]));
        assert!(matches!(
            create_smart_playlist(&conn, "taken", "", "alice", &rules),
            Err(SmartError::NameTaken)
        ));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM smart_playlist_definitions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn preview_copy_snapshot_and_manual_edit_guards() {
        let conn = fixture();
        let mut rules_json = def(
            "all",
            vec![rule("library_id", "in", Some(json!(["lib1", "gone"])))],
        );
        rules_json["limit"] = json!({ "maxTracks": 2 });
        let rules = valid(rules_json);
        let p = preview(&conn, "alice", &rules).unwrap();
        assert_eq!(p.track_count, 2);
        assert_eq!(p.total_duration, 720.0);
        assert_eq!(
            p.tracks
                .iter()
                .map(|t| t.title.clone().unwrap())
                .collect::<Vec<_>>(),
            ["Arc", "Bloom"]
        );
        assert_eq!(p.missing_library_ids, ["gone"]);
        let none: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlists", [], |r| r.get(0))
            .unwrap();
        assert_eq!(none, 0, "previews store nothing");

        let row = create_smart_playlist(&conn, "Lib one", "", "alice", &rules).unwrap();
        let (copy, added) =
            save_as_ordinary(&conn, &row.id, "alice", "Lib one (copy)", "").unwrap();
        assert_eq!((copy.kind.as_str(), added), ("manual", 2));
        assert_eq!(member_titles(&conn, &copy.id), ["Arc", "Bloom"]);
        assert!(matches!(
            save_as_ordinary(&conn, &copy.id, "alice", "Nope", ""),
            Err(SmartError::NotFound)
        ));

        let snapshot = playlist_snapshot(&conn, &row.id, "alice", false)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.playlist.kind, "smart");
        assert_eq!(snapshot.tracks.len(), 2);
        assert_eq!(snapshot.membership_revision, Some(1));
        assert!(snapshot.refresh_error.is_none());
        let manual = playlist_snapshot(&conn, &copy.id, "alice", false)
            .unwrap()
            .unwrap();
        assert_eq!(manual.membership_revision, None);
        assert!(playlist_snapshot(&conn, &row.id, "bob", false)
            .unwrap()
            .is_none());

        let t5 = EntityId::Str("t5".into());
        assert!(matches!(
            playlists::add_track_to_playlist(&conn, &row.id, &t5, "alice"),
            Err(playlists::AddTrackError::SmartPlaylist)
        ));
        assert!(matches!(
            playlists::batch_add_tracks(&conn, &row.id, std::slice::from_ref(&t5), "alice"),
            Err(playlists::EditError::SmartPlaylist)
        ));
        assert!(matches!(
            playlists::remove_track_from_playlist(
                &conn,
                &row.id,
                &EntityId::Str("t1".into()),
                "alice"
            ),
            Err(playlists::EditError::SmartPlaylist)
        ));
        assert!(matches!(
            playlists::reorder_playlist_tracks(&conn, &row.id, &[], "alice"),
            Err(playlists::EditError::SmartPlaylist)
        ));
        assert!(
            playlists::add_track_to_playlist(&conn, &copy.id, &t5, "alice").is_ok(),
            "the copy is editable"
        );
        assert!(playlists::is_smart_playlist(&conn, &row.id).unwrap());
    }

    #[test]
    fn evaluation_stays_fast_on_a_large_library() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES ('alice', 'alice');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'Music');
             INSERT INTO artists(id, name) VALUES ('a1', 'Artist One'), ('a2', 'Artist Two');
             BEGIN;",
        )
        .unwrap();
        {
            let mut track = conn
                .prepare(
                    "INSERT INTO tracks(id, library_id, artist_id, title, file_path, genre, year, duration, added_at, bpm)
                     VALUES (?1, 'lib1', ?2, ?3, ?4, ?5, ?6, ?7, datetime('2026-11-01', ?8), ?9)",
                )
                .unwrap();
            let mut play = conn
                .prepare("INSERT INTO play_history(id, user_id, track_id, played_at) VALUES (?1, 'alice', ?2, datetime('2026-11-01', ?3))")
                .unwrap();
            let mut rate = conn
                .prepare(
                    "INSERT INTO track_ratings(user_id, track_id, rating) VALUES ('alice', ?1, ?2)",
                )
                .unwrap();
            for i in 0..60_000_i64 {
                let id = format!("t{i:05}");
                let genre =
                    ["Rock", "Electronic, Ambient", "Jazz", "Hip-Hop", ""][(i % 5) as usize];
                track
                    .execute(rusqlite::params![
                        id,
                        if i % 2 == 0 { "a1" } else { "a2" },
                        format!("Song {i}"),
                        format!("/m1/{i}.mp3"),
                        genre,
                        1960 + i % 60,
                        120 + i % 300,
                        format!("-{} days", i % 900),
                        80 + i % 100,
                    ])
                    .unwrap();
                if i % 3 == 0 {
                    play.execute(rusqlite::params![
                        format!("p{i}"),
                        id,
                        format!("-{} days", i % 400)
                    ])
                    .unwrap();
                }
                if i % 6 == 0 {
                    rate.execute(rusqlite::params![id, 0.5 + (i % 10) as f64 * 0.5])
                        .unwrap();
                }
            }
        }
        conn.execute_batch("COMMIT").unwrap();

        let simple = parse_definition(
            &def(
                "all",
                vec![
                    rule("genre", "is", Some(json!("Ambient"))),
                    rule("year", "between", Some(json!([1980, 1999]))),
                ],
            )
            .to_string(),
        )
        .unwrap();
        let mut personal_json = def(
            "all",
            vec![
                rule("rating", "gte", Some(json!(3))),
                rule("last_played_at", "not_in_last_days", Some(json!(30))),
                rule("artist", "contains", Some(json!("one"))),
            ],
        );
        personal_json["sort"] = json!([{ "field": "last_played_at", "direction": "asc" }]);
        personal_json["limit"] = json!({ "maxTracks": 1000 });
        let personal = parse_definition(&personal_json.to_string()).unwrap();

        for (name, definition) in [("simple", &simple), ("personal", &personal)] {
            evaluate(&conn, "alice", definition, NOW).unwrap();
            let start = std::time::Instant::now();
            let selection = evaluate(&conn, "alice", definition, NOW).unwrap();
            let elapsed = start.elapsed();
            eprintln!(
                "{name}: {} tracks in {elapsed:?}",
                selection.track_ids.len()
            );
            assert!(!selection.track_ids.is_empty(), "{name}");
            assert!(elapsed < Duration::from_secs(2), "{name} took {elapsed:?}");
        }
    }

    #[test]
    fn smart_mix_jobs_keep_the_tracks_picked_at_enqueue() {
        use crate::boogiemix::{enqueue_mix_job, load_mix_job_inputs};
        let conn = fixture();
        let electronic = valid(def(
            "all",
            vec![rule("genre", "is", Some(json!("Electronic")))],
        ));
        let row = create_smart_playlist(&conn, "Mix me", "", "alice", &electronic).unwrap();
        let alice = EntityId::Str("alice".into());
        let enqueue = |conn: &Connection, playlist: &EntityId| {
            enqueue_mix_job(
                conn,
                playlist,
                &alice,
                8,
                "club_blend",
                "standard",
                "playlist",
                None,
            )
        };
        let job = enqueue(&conn, &row.id).unwrap();
        let (snapshot, membership): (i64, i64) = conn
            .query_row(
                "SELECT input_snapshot, source_membership_revision FROM mix_jobs WHERE id = ?1",
                [&job],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((snapshot, membership), (1, 1));
        let inputs = |conn: &Connection, job: &EntityId| -> Option<Vec<String>> {
            load_mix_job_inputs(conn, job, &row.id)
                .unwrap()
                .map(|tracks| {
                    tracks
                        .into_iter()
                        .map(|t| t.title.unwrap_or_default())
                        .collect()
                })
        };
        assert_eq!(inputs(&conn, &job).unwrap(), ["Arc", "Bloom"]);

        // A refresh that changes the playlist leaves the accepted job alone.
        conn.execute("UPDATE tracks SET genre = 'Rock' WHERE id = 't1'", [])
            .unwrap();
        ensure_fresh(&conn, &row.id, "alice", false).unwrap();
        assert_eq!(member_titles(&conn, &row.id), ["Bloom"]);
        assert_eq!(inputs(&conn, &job).unwrap(), ["Arc", "Bloom"]);

        // Too few tracks now: refused, and nothing is left behind.
        assert!(matches!(
            enqueue(&conn, &row.id),
            Err(crate::jobs::JobError::EmptyFolders)
        ));
        let jobs: i64 = conn
            .query_row("SELECT COUNT(*) FROM mix_jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jobs, 1);

        // A snapshot track that moved fails the job instead of mixing something else.
        conn.execute(
            "UPDATE tracks SET file_path = '/m1/2-moved' WHERE id = 't2'",
            [],
        )
        .unwrap();
        assert_eq!(inputs(&conn, &job), None);
        conn.execute("DELETE FROM mix_job_tracks WHERE job_id = ?1", [&job])
            .unwrap();
        assert_eq!(
            inputs(&conn, &job),
            None,
            "an empty snapshot never falls back"
        );

        // Manual playlists keep reading their current tracks; removing a job removes its snapshot.
        let manual = playlists::create_playlist(&conn, "By hand", "", "alice").unwrap();
        playlists::batch_add_tracks(
            &conn,
            &manual.id,
            &[EntityId::Str("t4".into()), EntityId::Str("t5".into())],
            "alice",
        )
        .unwrap();
        let manual_job = enqueue(&conn, &manual.id).unwrap();
        let manual_inputs = load_mix_job_inputs(&conn, &manual_job, &manual.id)
            .unwrap()
            .unwrap();
        assert_eq!(manual_inputs.len(), 2);
        let job2 = {
            conn.execute("UPDATE tracks SET genre = 'Electronic' WHERE id = 't4'", [])
                .unwrap();
            enqueue(&conn, &row.id).unwrap()
        };
        conn.execute("DELETE FROM mix_jobs WHERE id = ?1", [&job2])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mix_job_tracks WHERE job_id = ?1",
                [&job2],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }
}
