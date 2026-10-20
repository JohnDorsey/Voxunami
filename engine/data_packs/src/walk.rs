//! Walking a parsed JSON file while remembering where we are, so every complaint can name the path, and
//! collecting complaints instead of stopping at the first.

use serde_json::{Map, Value};

use crate::error::{PackError, PackErrors};

/// Reads one file: its name, and what has been found wrong with it so far.
pub(crate) struct Walk {
    file: String,
    errors: Vec<PackError>,
}

/// A JSON pointer: `/states/2/boxes`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Path(String);

impl Path {
    pub(crate) fn root() -> Self {
        Self::default()
    }

    pub(crate) fn key(&self, key: &str) -> Self {
        // JSON pointers escape `~` and `/` in keys.
        Self(format!(
            "{}/{}",
            self.0,
            key.replace('~', "~0").replace('/', "~1")
        ))
    }

    pub(crate) fn index(&self, index: usize) -> Self {
        Self(format!("{}/{index}", self.0))
    }

    fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

impl Walk {
    /// Parses `text` as JSON. A file that is not JSON is one error saying where.
    pub(crate) fn parse(file: &str, text: &str) -> Result<(Self, Value), PackErrors> {
        let walk = Self {
            file: file.to_owned(),
            errors: Vec::new(),
        };
        match serde_json::from_str::<Value>(text) {
            Ok(value) => Ok((walk, value)),
            Err(error) => Err(PackErrors(vec![PackError {
                file: file.to_owned(),
                path: String::new(),
                message: format!(
                    "this is not JSON (line {}, column {}): {}",
                    error.line(),
                    error.column(),
                    describe_syntax_error(&error)
                ),
            }])),
        }
    }

    pub(crate) fn fail(&mut self, path: &Path, message: impl Into<String>) {
        self.errors.push(PackError {
            file: self.file.clone(),
            path: path.0.clone(),
            message: message.into(),
        });
    }

    /// Done: the value if nothing was wrong, otherwise everything that was.
    pub(crate) fn finish<T>(self, value: T) -> Result<T, PackErrors> {
        if self.errors.is_empty() {
            Ok(value)
        } else {
            Err(PackErrors(self.errors))
        }
    }

    /// The value as an object whose keys are among `allowed`; others are errors, suggesting the nearest allowed
    /// key if one is close.
    pub(crate) fn object<'a>(
        &mut self,
        value: &'a Value,
        path: &Path,
        allowed: &[&str],
    ) -> Option<&'a Map<String, Value>> {
        let Some(object) = value.as_object() else {
            self.fail(
                path,
                format!("expected an object, found {}", kind_of(value)),
            );
            return None;
        };
        for key in object.keys() {
            // Editors read the schema a file names in `$schema`; it says nothing about the block.
            if path.is_root() && key == "$schema" {
                continue;
            }
            if !allowed.contains(&key.as_str()) {
                let hint = nearest(key, allowed)
                    .map(|near| format!(" (did you mean \"{near}\"?)"))
                    .unwrap_or_default();
                self.fail(
                    &path.key(key),
                    format!(
                        "unknown key \"{key}\"{hint}; the keys here are {}",
                        quoted(allowed)
                    ),
                );
            }
        }
        Some(object)
    }

    pub(crate) fn require<'a>(
        &mut self,
        object: &'a Map<String, Value>,
        key: &str,
        path: &Path,
    ) -> Option<&'a Value> {
        let found = object.get(key);
        if found.is_none() {
            self.fail(path, format!("missing \"{key}\""));
        }
        found
    }

    pub(crate) fn boolean(&mut self, value: &Value, path: &Path) -> Option<bool> {
        let found = value.as_bool();
        if found.is_none() {
            self.fail(
                path,
                format!("expected true or false, found {}", kind_of(value)),
            );
        }
        found
    }

    pub(crate) fn string<'a>(&mut self, value: &'a Value, path: &Path) -> Option<&'a str> {
        let found = value.as_str();
        if found.is_none() {
            self.fail(path, format!("expected text, found {}", kind_of(value)));
        }
        found
    }

    pub(crate) fn array<'a>(&mut self, value: &'a Value, path: &Path) -> Option<&'a Vec<Value>> {
        let found = value.as_array();
        if found.is_none() {
            self.fail(path, format!("expected a list, found {}", kind_of(value)));
        }
        found
    }

    /// A whole number from `minimum` to `maximum`.
    pub(crate) fn integer(
        &mut self,
        value: &Value,
        path: &Path,
        minimum: i64,
        maximum: i64,
    ) -> Option<i64> {
        let Some(number) = whole_number(value) else {
            self.fail(
                path,
                format!("expected a whole number, found {}", describe(value)),
            );
            return None;
        };
        if !(minimum..=maximum).contains(&number) {
            self.fail(path, format!("{number} is outside {minimum} to {maximum}"));
            return None;
        }
        Some(number)
    }

    /// Three numbers from 0 to 1.
    pub(crate) fn color(&mut self, value: &Value, path: &Path) -> Option<[f32; 3]> {
        let list = self.array(value, path)?;
        if list.len() != 3 {
            self.fail(
                path,
                format!(
                    "a colour is 3 numbers (red, green, blue), found {}",
                    list.len()
                ),
            );
            return None;
        }
        let mut color = [0.0f32; 3];
        let mut good = true;
        for (index, (slot, component)) in color.iter_mut().zip(list).enumerate() {
            match component.as_f64() {
                Some(number) if (0.0..=1.0).contains(&number) => *slot = number as f32,
                Some(number) => {
                    self.fail(&path.index(index), format!("{number} is outside 0 to 1"));
                    good = false;
                }
                None => {
                    self.fail(
                        &path.index(index),
                        format!("expected a number, found {}", kind_of(component)),
                    );
                    good = false;
                }
            }
        }
        good.then_some(color)
    }
}

/// A JSON number that is a whole number, written as `3` or as `3.0` (JSON Schema's `integer` takes both).
fn whole_number(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.fract() == 0.0 && number.abs() < 9.0e15)
            .map(|number| number as i64)
    })
}

/// What a value is, for "found ...".
pub(crate) fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// A value shown briefly, for a message about a wrong number.
fn describe(value: &Value) -> String {
    match value {
        Value::Number(number) => number.to_string(),
        other => kind_of(other).to_owned(),
    }
}

pub(crate) fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The word in `choices` nearest to `word`, if one is near enough to be what was meant.
pub(crate) fn nearest<'a>(word: &str, choices: &[&'a str]) -> Option<&'a str> {
    choices
        .iter()
        .map(|choice| (edit_distance(word, choice), *choice))
        .filter(|(distance, choice)| *distance <= 2 && *distance < choice.len())
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, choice)| choice)
}

fn edit_distance(first: &str, second: &str) -> usize {
    let second: Vec<char> = second.chars().collect();
    let mut row: Vec<usize> = (0..=second.len()).collect();
    for (i, a) in first.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, b) in second.iter().enumerate() {
            let held = row[j + 1];
            row[j + 1] = (previous + usize::from(a != *b))
                .min(row[j] + 1)
                .min(held + 1);
            previous = held;
        }
    }
    row[second.len()]
}

/// The parser's own wording says what it expected; its repeated position is dropped.
fn describe_syntax_error(error: &serde_json::Error) -> String {
    let text = error.to_string();
    match text.rfind(" at line ") {
        Some(index) => text[..index].to_owned(),
        None => text,
    }
}
