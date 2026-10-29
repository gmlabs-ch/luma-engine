//! Script values: the dynamic type of the script language, with the
//! conversions, comparisons and property rules of the original engine.
//!
//! Written with reference to `ScValue.cpp` and `BScriptable.cpp` of WME
//! Lite (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.
//!
//! A [`Value`] is one of `null`, a boolean, a 32-bit integer, a 64-bit
//! float, a string, a script object (a map of properties) or a handle to a
//! native object of the engine. Objects have **value semantics**: copying
//! a value copies its properties, as the original engine does. Natives are
//! shared handles; what they convert to, how they compare and which
//! properties they expose is answered by the host through the [`Natives`]
//! trait.
//!
//! References to variables (`PUSH_VAR_REF`) are not values: the virtual
//! machine keeps them on its stacks and dereferences them before calling
//! anything here.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

/// A handle to a native object of the engine, issued by the host. Two
/// handles are the same object when they are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NativeId(pub u64);

/// The properties of a script object, in the order the original engine
/// keeps them (sorted by name).
pub type Properties = BTreeMap<String, Value>;

/// What the host tells the script layer about its native objects.
///
/// The defaults are those of the engine's base class: a native converts
/// to `"[native object]"`, `0`, `0.0` and `false`, compares by identity,
/// and accepts no property.
pub trait Natives {
    /// The class name, used to decide whether two natives are comparable.
    fn class_name(&self, id: NativeId) -> String;

    /// The string form (`ScToString`).
    fn to_string(&self, _id: NativeId) -> String {
        "[native object]".to_string()
    }

    /// The integer form (`ScToInt`).
    fn to_int(&self, _id: NativeId) -> i32 {
        0
    }

    /// The float form (`ScToFloat`).
    fn to_float(&self, _id: NativeId) -> f64 {
        0.0
    }

    /// The boolean form (`ScToBool`).
    fn to_bool(&self, _id: NativeId) -> bool {
        false
    }

    /// Orders two natives of the same class (`ScCompare`).
    fn compare(&self, a: NativeId, b: NativeId) -> Ordering {
        a.cmp(&b)
    }

    /// Reads a property (`ScGetProperty`), built-in or set by a script.
    fn get_property(&self, id: NativeId, name: &str) -> Option<Value>;

    /// Writes a property (`ScSetProperty`). Returns `false` when the
    /// native refuses it, which the engine's objects never do: they keep
    /// the properties scripts set on them.
    fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool;

    /// A scalar assigned to a variable holding this native (`ScSetInt`,
    /// `ScSetString`, `ScSetBool`): the variable keeps the native, and
    /// the native may take the value, as the `String` class does. The
    /// default ignores it.
    fn assign_scalar(&mut self, _id: NativeId, _value: &Value) {}
}

/// A host without native objects, for tests and tools.
impl Natives for () {
    fn class_name(&self, _id: NativeId) -> String {
        String::new()
    }

    fn get_property(&self, _id: NativeId, _name: &str) -> Option<Value> {
        None
    }

    fn set_property(&mut self, _id: NativeId, _name: &str, _value: &Value) -> bool {
        false
    }
}

/// A script value.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Value {
    /// `null`, also the value of an undefined variable.
    #[default]
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A 32-bit integer.
    Int(i32),
    /// A 64-bit float.
    Float(f64),
    /// A string of 8-bit characters (kept as `char`s below U+0100).
    String(String),
    /// A script object: its properties.
    Object(Properties),
    /// A native object of the engine.
    Native(NativeId),
}

impl Value {
    /// A string value.
    pub fn string(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    /// An empty object (`new Object()`).
    pub fn object() -> Value {
        Value::Object(Properties::new())
    }

    /// The name of the type, for messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Object(_) => "object",
            Value::Native(_) => "native",
        }
    }

    /// Whether the two values have the same type (not the same value).
    pub fn same_type(&self, other: &Value) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Whether this is a string.
    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    /// Whether this is a float.
    pub fn is_float(&self) -> bool {
        matches!(self, Value::Float(_))
    }

    /// Whether this is a script object.
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    /// Whether this is a native object.
    pub fn is_native(&self) -> bool {
        matches!(self, Value::Native(_))
    }

    /// The native handle, if this is a native.
    pub fn as_native(&self) -> Option<NativeId> {
        match self {
            Value::Native(id) => Some(*id),
            _ => None,
        }
    }

    /// The string, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The value as a boolean (`GetBool`): numbers are true when non-zero,
    /// strings when they read `1`, `yes` or `true` (any case), `null` and
    /// objects are false.
    pub fn get_bool(&self, natives: &dyn Natives) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => {
                s.eq_ignore_ascii_case("1")
                    || s.eq_ignore_ascii_case("yes")
                    || s.eq_ignore_ascii_case("true")
            }
            Value::Native(id) => natives.to_bool(*id),
            Value::Null | Value::Object(_) => false,
        }
    }

    /// The value as an integer (`GetInt`): floats are truncated, strings
    /// read like C's `atoi`, `null` and objects are 0.
    pub fn get_int(&self, natives: &dyn Natives) -> i32 {
        match self {
            Value::Bool(b) => i32::from(*b),
            Value::Int(i) => *i,
            Value::Float(f) => *f as i32,
            Value::String(s) => atoi(s),
            Value::Native(id) => natives.to_int(*id),
            Value::Null | Value::Object(_) => 0,
        }
    }

    /// The value as a float (`GetFloat`): strings read like C's `atof`,
    /// `null` and objects are 0.
    pub fn get_float(&self, natives: &dyn Natives) -> f64 {
        match self {
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Int(i) => f64::from(*i),
            Value::Float(f) => *f,
            Value::String(s) => atof(s),
            Value::Native(id) => natives.to_float(*id),
            Value::Null | Value::Object(_) => 0.0,
        }
    }

    /// The value as a string (`GetString`): booleans are `yes` and `no`,
    /// floats are printed like C's `%f`, `null` is `[null]`, objects are
    /// `[object]`.
    pub fn get_string(&self, natives: &dyn Natives) -> String {
        match self {
            Value::Null => "[null]".to_string(),
            Value::Bool(true) => "yes".to_string(),
            Value::Bool(false) => "no".to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => format_float(*f),
            Value::String(s) => s.clone(),
            Value::Object(_) => "[object]".to_string(),
            Value::Native(id) => natives.to_string(*id),
        }
    }

    /// Orders two values for `==`, `<` and the like (`Compare`):
    ///
    /// - two natives of the same class ask the host, of different classes
    ///   compare as strings;
    /// - two objects are never equal (the first is "less");
    /// - `null` is less than anything but `null`;
    /// - if either is a string, both compare as strings (byte order);
    /// - if either is a float, both compare as floats;
    /// - otherwise both compare as integers.
    pub fn compare(&self, other: &Value, natives: &dyn Natives) -> Ordering {
        if let (Value::Native(a), Value::Native(b)) = (self, other) {
            if natives.class_name(*a) == natives.class_name(*b) {
                return natives.compare(*a, *b);
            }
            return natives.to_string(*a).cmp(&natives.to_string(*b));
        }
        if self.is_object() && other.is_object() {
            return Ordering::Less;
        }
        match (self.is_null(), other.is_null()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        if self.is_string() || other.is_string() {
            return self.get_string(natives).cmp(&other.get_string(natives));
        }
        if self.is_float() || other.is_float() {
            return self
                .get_float(natives)
                .partial_cmp(&other.get_float(natives))
                .unwrap_or(Ordering::Equal);
        }
        self.get_int(natives).cmp(&other.get_int(natives))
    }

    /// Orders two values for `===` and `!==` (`CompareStrict`): values of
    /// different types are never equal, values of the same type compare
    /// as with [`Value::compare`].
    pub fn compare_strict(&self, other: &Value, natives: &dyn Natives) -> Ordering {
        if self.same_type(other) {
            self.compare(other, natives)
        } else {
            Ordering::Less
        }
    }

    /// `a + b`: `null` if either is `null`, concatenation if either is a
    /// string, an integer sum if both are integers, a float sum
    /// otherwise.
    pub fn add(&self, other: &Value, natives: &dyn Natives) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (Value::String(_), _) | (_, Value::String(_)) => {
                Value::String(self.get_string(natives) + &other.get_string(natives))
            }
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_add(*b)),
            _ => Value::Float(self.get_float(natives) + other.get_float(natives)),
        }
    }

    /// `a - b`: `null` if either is `null`, an integer difference if both
    /// are integers, a float difference otherwise.
    pub fn sub(&self, other: &Value, natives: &dyn Natives) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_sub(*b)),
            _ => Value::Float(self.get_float(natives) - other.get_float(natives)),
        }
    }

    /// `a * b`: `null` if either is `null`, an integer product if both are
    /// integers, a float product otherwise.
    pub fn mul(&self, other: &Value, natives: &dyn Natives) -> Value {
        match (self, other) {
            (Value::Null, _) | (_, Value::Null) => Value::Null,
            (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_mul(*b)),
            _ => Value::Float(self.get_float(natives) * other.get_float(natives)),
        }
    }

    /// `a / b`, always a float. `Err(DivisionByZero)` when `b` is zero
    /// (the engine reports a runtime error and uses `null`); `Ok(null)`
    /// when either is `null`.
    pub fn div(&self, other: &Value, natives: &dyn Natives) -> Result<Value, DivisionByZero> {
        let divisor = other.get_float(natives);
        if divisor == 0.0 {
            return Err(DivisionByZero);
        }
        if self.is_null() || other.is_null() {
            return Ok(Value::Null);
        }
        Ok(Value::Float(self.get_float(natives) / divisor))
    }

    /// `a % b` on integers. `Err(DivisionByZero)` when `b` converts to 0;
    /// `Ok(null)` when either is `null`.
    pub fn modulo(&self, other: &Value, natives: &dyn Natives) -> Result<Value, DivisionByZero> {
        let divisor = other.get_int(natives);
        if divisor == 0 {
            return Err(DivisionByZero);
        }
        if self.is_null() || other.is_null() {
            return Ok(Value::Null);
        }
        Ok(Value::Int(self.get_int(natives).wrapping_rem(divisor)))
    }

    /// `!a`: `null` is true, otherwise the negated boolean form.
    pub fn not(&self, natives: &dyn Natives) -> bool {
        self.is_null() || !self.get_bool(natives)
    }

    /// A property of this value (`GetProp`): the `Length` of a string, a
    /// property of a native (asked to the host) or of an object.
    pub fn get_property(&self, name: &str, natives: &dyn Natives) -> Option<Value> {
        match self {
            Value::String(s) if name == "Length" => Some(Value::Int(s.chars().count() as i32)),
            Value::Native(id) => natives.get_property(*id, name),
            Value::Object(props) => props.get(name).cloned(),
            _ => None,
        }
    }

    /// Sets a property (`SetProp`). On a native the host stores it; on any
    /// other value the value becomes an object holding it (a scalar loses
    /// its old content). Returns `false` only when a native refuses the
    /// property.
    pub fn set_property(&mut self, name: &str, value: Value, natives: &mut dyn Natives) -> bool {
        if let Value::Native(id) = self {
            return natives.set_property(*id, name, &value);
        }
        if !self.is_object() {
            *self = Value::object();
        }
        if let Value::Object(props) = self {
            props.insert(name.to_string(), value);
        }
        true
    }

    /// Whether an object has this property (`PropExists`). Natives and
    /// scalars have none: the host's properties are not consulted, as in
    /// the engine's variable lookup.
    pub fn has_property(&self, name: &str) -> bool {
        match self {
            Value::Object(props) => props.contains_key(name),
            _ => false,
        }
    }

    /// Removes a property of an object (`DeleteProp`), returning it.
    pub fn remove_property(&mut self, name: &str) -> Option<Value> {
        match self {
            Value::Object(props) => props.remove(name),
            _ => None,
        }
    }

    /// The properties of an object.
    pub fn properties(&self) -> Option<&Properties> {
        match self {
            Value::Object(props) => Some(props),
            _ => None,
        }
    }

    /// Assigns `from` to this variable (`SetValue`): a variable holding a
    /// native that is assigned an integer, string or boolean keeps the
    /// native and hands it the scalar; anything else replaces the value.
    pub fn assign(&mut self, from: &Value, natives: &mut dyn Natives) {
        if let Value::Native(id) = self {
            if matches!(from, Value::Int(_) | Value::String(_) | Value::Bool(_)) {
                natives.assign_scalar(*id, from);
                return;
            }
        }
        *self = from.clone();
    }
}

/// Division or modulo by zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DivisionByZero;

impl fmt::Display for DivisionByZero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("division by zero")
    }
}

impl std::error::Error for DivisionByZero {}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Value {
        Value::Int(i)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Value {
        Value::Float(f)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(s)
    }
}

impl From<NativeId> for Value {
    fn from(id: NativeId) -> Value {
        Value::Native(id)
    }
}

fn is_c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r')
}

/// C's `atoi`: leading whitespace, an optional sign, then digits; anything
/// else ends the number, and no digits means 0. Overflow wraps like a
/// 64-bit `strtol` result cast to `int`.
pub fn atoi(s: &str) -> i32 {
    let s = s.trim_start_matches(is_c_space);
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let mut value: i64 = 0;
    for c in digits.chars() {
        let Some(d) = c.to_digit(10) else { break };
        value = value.saturating_mul(10).saturating_add(if negative {
            -i64::from(d)
        } else {
            i64::from(d)
        });
    }
    value as i32
}

/// C's `atof`: leading whitespace, then the longest prefix that is a
/// decimal number with an optional fraction and exponent, or `inf`,
/// `infinity` or `nan` in any case; no number means 0.
pub fn atof(s: &str) -> f64 {
    let s = s.trim_start_matches(is_c_space);
    let (sign, rest) = match s.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, s.strip_prefix('+').unwrap_or(s)),
    };
    let lower = rest.to_ascii_lowercase();
    if lower.starts_with("inf") {
        return sign * f64::INFINITY;
    }
    if lower.starts_with("nan") {
        return sign * f64::NAN;
    }
    let bytes = rest.as_bytes();
    let mut end = 0;
    let digits = |from: usize| -> usize {
        let mut i = from;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut seen_digit = false;
    let after_int = digits(0);
    seen_digit |= after_int > 0;
    end = end.max(after_int);
    if bytes.get(end) == Some(&b'.') {
        let after_frac = digits(end + 1);
        if after_frac > end + 1 {
            seen_digit = true;
            end = after_frac;
        } else if seen_digit {
            end += 1;
        }
    }
    if !seen_digit {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut i = end + 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let after_exp = digits(i);
        if after_exp > i {
            end = after_exp;
        }
    }
    sign * rest[..end].parse::<f64>().unwrap_or(0.0)
}

/// C's `%f`: six decimals, `inf`, `-inf` and `nan`.
pub fn format_float(f: f64) -> String {
    if f.is_nan() {
        if f.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        }
    } else if f.is_infinite() {
        if f < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        }
    } else {
        format!("{f:.6}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A host with natives of two classes: `String` natives convert to
    /// their text and take assigned scalars; `Entity` natives keep script
    /// properties and expose a built-in `Name`.
    #[derive(Default)]
    struct Host {
        strings: HashMap<NativeId, String>,
        entities: HashMap<NativeId, (String, Properties)>,
    }

    impl Host {
        fn string(&mut self, id: u64, text: &str) -> Value {
            self.strings.insert(NativeId(id), text.to_string());
            Value::Native(NativeId(id))
        }

        fn entity(&mut self, id: u64, name: &str) -> Value {
            self.entities
                .insert(NativeId(id), (name.to_string(), Properties::new()));
            Value::Native(NativeId(id))
        }
    }

    impl Natives for Host {
        fn class_name(&self, id: NativeId) -> String {
            if self.strings.contains_key(&id) {
                "String".to_string()
            } else {
                "Entity".to_string()
            }
        }

        fn to_string(&self, id: NativeId) -> String {
            match self.strings.get(&id) {
                Some(s) => s.clone(),
                None => "[entity]".to_string(),
            }
        }

        fn to_int(&self, id: NativeId) -> i32 {
            self.strings.get(&id).map(|s| atoi(s)).unwrap_or(0)
        }

        fn to_bool(&self, id: NativeId) -> bool {
            self.entities.contains_key(&id)
        }

        fn compare(&self, a: NativeId, b: NativeId) -> Ordering {
            match (self.strings.get(&a), self.strings.get(&b)) {
                (Some(x), Some(y)) => x.cmp(y),
                _ => a.cmp(&b),
            }
        }

        fn get_property(&self, id: NativeId, name: &str) -> Option<Value> {
            let (entity_name, props) = self.entities.get(&id)?;
            if name == "Name" {
                return Some(Value::string(entity_name));
            }
            props.get(name).cloned()
        }

        fn set_property(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
            match self.entities.get_mut(&id) {
                Some((_, props)) => {
                    props.insert(name.to_string(), value.clone());
                    true
                }
                None => false,
            }
        }

        fn assign_scalar(&mut self, id: NativeId, value: &Value) {
            if let Some(s) = self.strings.get_mut(&id) {
                *s = value.get_string(&());
            }
        }
    }

    #[test]
    fn converts_to_bool_like_the_engine() {
        let n = &();
        assert!(!Value::Null.get_bool(n));
        assert!(Value::Bool(true).get_bool(n));
        assert!(Value::Int(-3).get_bool(n));
        assert!(!Value::Int(0).get_bool(n));
        assert!(Value::Float(0.5).get_bool(n));
        assert!(!Value::Float(0.0).get_bool(n));
        for text in ["1", "yes", "YES", "True", "tRuE"] {
            assert!(Value::from(text).get_bool(n), "{text}");
        }
        for text in ["0", "no", "false", "", "2", " yes", "truex"] {
            assert!(!Value::from(text).get_bool(n), "{text}");
        }
        assert!(!Value::object().get_bool(n));
        assert!(!Value::Native(NativeId(1)).get_bool(n));
    }

    #[test]
    fn converts_to_int_like_the_engine() {
        let n = &();
        assert_eq!(Value::Null.get_int(n), 0);
        assert_eq!(Value::Bool(true).get_int(n), 1);
        assert_eq!(Value::Float(3.99).get_int(n), 3);
        assert_eq!(Value::Float(-3.99).get_int(n), -3);
        assert_eq!(Value::Float(1e20).get_int(n), i32::MAX);
        assert_eq!(Value::Float(f64::NAN).get_int(n), 0);
        assert_eq!(Value::from("42abc").get_int(n), 42);
        assert_eq!(Value::from("  -17").get_int(n), -17);
        assert_eq!(Value::from("+8").get_int(n), 8);
        assert_eq!(Value::from("abc").get_int(n), 0);
        assert_eq!(Value::from("3.9").get_int(n), 3);
        assert_eq!(Value::from("").get_int(n), 0);
        assert_eq!(Value::from("4294967297").get_int(n), 1);
        assert_eq!(Value::object().get_int(n), 0);
    }

    #[test]
    fn converts_to_float_like_the_engine() {
        let n = &();
        assert_eq!(Value::Null.get_float(n), 0.0);
        assert_eq!(Value::Bool(true).get_float(n), 1.0);
        assert_eq!(Value::Int(-2).get_float(n), -2.0);
        assert_eq!(Value::from("1.5e2x").get_float(n), 150.0);
        assert_eq!(Value::from("1.5e").get_float(n), 1.5);
        assert_eq!(Value::from("1.5e+").get_float(n), 1.5);
        assert_eq!(Value::from(".5").get_float(n), 0.5);
        assert_eq!(Value::from("5.").get_float(n), 5.0);
        assert_eq!(Value::from("-.5").get_float(n), -0.5);
        assert_eq!(Value::from("\t 7 8").get_float(n), 7.0);
        assert_eq!(Value::from(".").get_float(n), 0.0);
        assert_eq!(Value::from("-").get_float(n), 0.0);
        assert_eq!(Value::from("e5").get_float(n), 0.0);
        assert_eq!(Value::from("abc").get_float(n), 0.0);
        assert_eq!(Value::from("INF").get_float(n), f64::INFINITY);
        assert_eq!(Value::from("-Infinity").get_float(n), f64::NEG_INFINITY);
        assert!(Value::from("nan").get_float(n).is_nan());
        assert_eq!(Value::object().get_float(n), 0.0);
    }

    #[test]
    fn converts_to_string_like_the_engine() {
        let n = &();
        assert_eq!(Value::Null.get_string(n), "[null]");
        assert_eq!(Value::Bool(true).get_string(n), "yes");
        assert_eq!(Value::Bool(false).get_string(n), "no");
        assert_eq!(Value::Int(-12).get_string(n), "-12");
        assert_eq!(Value::Float(1.5).get_string(n), "1.500000");
        assert_eq!(Value::Float(-0.0).get_string(n), "-0.000000");
        assert_eq!(Value::Float(1e10).get_string(n), "10000000000.000000");
        assert_eq!(Value::Float(f64::INFINITY).get_string(n), "inf");
        assert_eq!(Value::Float(f64::NEG_INFINITY).get_string(n), "-inf");
        assert_eq!(Value::Float(f64::NAN).get_string(n), "nan");
        assert_eq!(Value::Float(-f64::NAN).get_string(n), "-nan");
        assert_eq!(Value::from("héllo").get_string(n), "héllo");
        assert_eq!(Value::object().get_string(n), "[object]");
        assert_eq!(Value::Native(NativeId(9)).get_string(n), "[native object]");
    }

    #[test]
    fn natives_convert_through_the_host() {
        let mut host = Host::default();
        let s = host.string(1, "12 apples");
        let e = host.entity(2, "crate");
        assert_eq!(s.get_string(&host), "12 apples");
        assert_eq!(s.get_int(&host), 12);
        assert_eq!(s.get_float(&host), 0.0);
        assert!(!s.get_bool(&host));
        assert_eq!(e.get_string(&host), "[entity]");
        assert!(e.get_bool(&host));
    }

    #[test]
    fn compares_like_the_engine() {
        let n = &();
        let cmp = |a: Value, b: Value| a.compare(&b, n);
        // Null.
        assert_eq!(cmp(Value::Null, Value::Null), Ordering::Equal);
        assert_eq!(cmp(Value::Null, Value::Int(0)), Ordering::Less);
        assert_eq!(cmp(Value::Int(0), Value::Null), Ordering::Greater);
        assert_eq!(cmp(Value::Null, Value::from("")), Ordering::Less);
        // Objects never equal.
        assert_eq!(cmp(Value::object(), Value::object()), Ordering::Less);
        // Strings: byte order, numbers converted.
        assert_eq!(cmp(Value::from("abc"), Value::from("abd")), Ordering::Less);
        assert_eq!(cmp(Value::from("B"), Value::from("a")), Ordering::Less);
        assert_eq!(cmp(Value::Int(10), Value::from("9")), Ordering::Less);
        assert_eq!(cmp(Value::from("10"), Value::Int(10)), Ordering::Equal);
        assert_eq!(cmp(Value::Bool(true), Value::from("yes")), Ordering::Equal);
        assert_eq!(
            cmp(Value::Float(1.5), Value::from("1.500000")),
            Ordering::Equal
        );
        assert_eq!(cmp(Value::from("é"), Value::from("z")), Ordering::Greater);
        // Floats.
        assert_eq!(cmp(Value::Float(1.5), Value::Int(1)), Ordering::Greater);
        assert_eq!(cmp(Value::Int(2), Value::Float(2.0)), Ordering::Equal);
        assert_eq!(
            cmp(Value::Float(f64::NAN), Value::Float(1.0)),
            Ordering::Equal
        );
        // Integers, with booleans.
        assert_eq!(cmp(Value::Int(1), Value::Bool(true)), Ordering::Equal);
        assert_eq!(cmp(Value::Bool(false), Value::Int(1)), Ordering::Less);
        assert_eq!(cmp(Value::Int(-1), Value::Int(1)), Ordering::Less);
        // An object against a scalar compares as integers (0).
        assert_eq!(cmp(Value::object(), Value::Int(0)), Ordering::Equal);
    }

    #[test]
    fn compares_natives_by_class() {
        let mut host = Host::default();
        let a = host.string(1, "apple");
        let b = host.string(2, "banana");
        let e = host.entity(3, "crate");
        assert_eq!(a.compare(&b, &host), Ordering::Less);
        assert_eq!(b.compare(&a, &host), Ordering::Greater);
        assert_eq!(a.compare(&a, &host), Ordering::Equal);
        // Different classes: by string form ("[entity]" < "apple").
        assert_eq!(e.compare(&a, &host), Ordering::Less);
        // A native against a string: strings.
        assert_eq!(a.compare(&Value::from("apple"), &host), Ordering::Equal);
        // A native against an int: ints (the entity converts to 0).
        assert_eq!(e.compare(&Value::Int(0), &host), Ordering::Equal);
        // Two entities: identity.
        let f = host.entity(4, "other");
        assert_eq!(e.compare(&f, &host), Ordering::Less);
        assert_eq!(f.compare(&e, &host), Ordering::Greater);
    }

    #[test]
    fn strict_comparison_requires_the_same_type() {
        let n = &();
        assert_eq!(
            Value::Int(1).compare_strict(&Value::Float(1.0), n),
            Ordering::Less
        );
        assert_eq!(
            Value::Int(1).compare_strict(&Value::Int(1), n),
            Ordering::Equal
        );
        assert_eq!(
            Value::from("1").compare_strict(&Value::Int(1), n),
            Ordering::Less
        );
        assert_eq!(Value::Null.compare_strict(&Value::Null, n), Ordering::Equal);
        assert_eq!(
            Value::Bool(true).compare_strict(&Value::Int(1), n),
            Ordering::Less
        );
        assert_eq!(
            Value::Float(2.0).compare_strict(&Value::Float(2.0), n),
            Ordering::Equal
        );
    }

    #[test]
    fn arithmetic_follows_the_engine() {
        let n = &();
        assert_eq!(Value::Int(2).add(&Value::Int(3), n), Value::Int(5));
        assert_eq!(Value::Int(2).add(&Value::Float(0.5), n), Value::Float(2.5));
        assert_eq!(Value::from("a").add(&Value::Int(1), n), Value::from("a1"));
        assert_eq!(Value::Int(1).add(&Value::from("a"), n), Value::from("1a"));
        assert_eq!(
            Value::Bool(true).add(&Value::from(""), n),
            Value::from("yes")
        );
        assert_eq!(Value::Null.add(&Value::from("a"), n), Value::Null);
        assert_eq!(Value::from("a").add(&Value::Null, n), Value::Null);
        assert_eq!(
            Value::Int(i32::MAX).add(&Value::Int(1), n),
            Value::Int(i32::MIN)
        );
        assert_eq!(
            Value::Bool(true).add(&Value::Bool(true), n),
            Value::Float(2.0)
        );

        assert_eq!(Value::Int(2).sub(&Value::Int(3), n), Value::Int(-1));
        assert_eq!(Value::from("5").sub(&Value::Int(3), n), Value::Float(2.0));
        assert_eq!(Value::Null.sub(&Value::Int(3), n), Value::Null);

        assert_eq!(Value::Int(2).mul(&Value::Int(3), n), Value::Int(6));
        assert_eq!(Value::Int(2).mul(&Value::Float(1.5), n), Value::Float(3.0));
        assert_eq!(Value::Int(2).mul(&Value::Null, n), Value::Null);

        assert_eq!(Value::Int(3).div(&Value::Int(2), n), Ok(Value::Float(1.5)));
        assert_eq!(Value::Int(3).div(&Value::Int(0), n), Err(DivisionByZero));
        assert_eq!(Value::Int(3).div(&Value::from("x"), n), Err(DivisionByZero));
        assert_eq!(Value::Null.div(&Value::Int(2), n), Ok(Value::Null));
        assert_eq!(Value::Int(3).div(&Value::Null, n), Err(DivisionByZero));

        assert_eq!(Value::Int(7).modulo(&Value::Int(3), n), Ok(Value::Int(1)));
        assert_eq!(Value::Int(-7).modulo(&Value::Int(3), n), Ok(Value::Int(-1)));
        assert_eq!(
            Value::Float(7.9).modulo(&Value::Int(3), n),
            Ok(Value::Int(1))
        );
        assert_eq!(
            Value::Int(7).modulo(&Value::Float(0.5), n),
            Err(DivisionByZero)
        );
        assert_eq!(Value::Null.modulo(&Value::Int(3), n), Ok(Value::Null));
        assert_eq!(
            Value::Int(i32::MIN).modulo(&Value::Int(-1), n),
            Ok(Value::Int(0))
        );

        assert!(Value::Null.not(n));
        assert!(!Value::Int(1).not(n));
        assert!(Value::Int(0).not(n));
        assert!(Value::from("no").not(n));
    }

    #[test]
    fn objects_hold_properties_by_value() {
        let n = &mut ();
        let mut a = Value::object();
        assert!(a.set_property("x", Value::Int(1), n));
        assert_eq!(a.get_property("x", n), Some(Value::Int(1)));
        assert_eq!(a.get_property("y", n), None);
        assert!(a.has_property("x"));
        assert!(!a.has_property("y"));
        // Copies are independent.
        let mut b = a.clone();
        b.set_property("x", Value::Int(2), n);
        assert_eq!(a.get_property("x", n), Some(Value::Int(1)));
        assert_eq!(b.get_property("x", n), Some(Value::Int(2)));
        // Sorted by name.
        a.set_property("b", Value::Null, n);
        a.set_property("A", Value::Null, n);
        let names: Vec<&str> = a.properties().unwrap().keys().map(String::as_str).collect();
        assert_eq!(names, ["A", "b", "x"]);
        assert_eq!(a.remove_property("x"), Some(Value::Int(1)));
        assert!(!a.has_property("x"));
        assert_eq!(a.remove_property("x"), None);
    }

    #[test]
    fn setting_a_property_on_a_scalar_makes_it_an_object() {
        let n = &mut ();
        let mut v = Value::Null;
        v.set_property("Visited", Value::Bool(false), n);
        assert!(v.is_object());
        assert_eq!(v.get_property("Visited", n), Some(Value::Bool(false)));
        let mut v = Value::Int(5);
        v.set_property("x", Value::Int(1), n);
        assert_eq!(v.get_string(n), "[object]");
        assert_eq!(v.get_int(n), 0);
        // Scalars have no properties, except a string's Length.
        assert_eq!(Value::Int(5).get_property("x", n), None);
        assert_eq!(
            Value::from("héllo").get_property("Length", n),
            Some(Value::Int(5))
        );
        assert_eq!(
            Value::from("").get_property("Length", n),
            Some(Value::Int(0))
        );
        assert_eq!(Value::from("abc").get_property("length", n), None);
        assert!(!Value::from("abc").has_property("Length"));
    }

    #[test]
    fn native_properties_live_in_the_host() {
        let mut host = Host::default();
        let mut e = host.entity(1, "crate");
        assert_eq!(e.get_property("Name", &host), Some(Value::from("crate")));
        assert_eq!(e.get_property("xResult", &host), None);
        assert!(e.set_property("xResult", Value::Int(7), &mut host));
        assert_eq!(e.get_property("xResult", &host), Some(Value::Int(7)));
        // A copy of the handle sees the same property.
        let copy = e.clone();
        assert_eq!(copy.get_property("xResult", &host), Some(Value::Int(7)));
        assert!(e.is_native());
        assert!(!e.has_property("xResult"));
        // A native that refuses keeps the value unchanged.
        let mut s = host.string(2, "text");
        assert!(!s.set_property("x", Value::Null, &mut host));
        assert!(s.is_native());
    }

    #[test]
    fn assignment_preserves_a_native_given_a_scalar() {
        let mut host = Host::default();
        let mut var = host.string(1, "old");
        var.assign(&Value::from("new"), &mut host);
        assert_eq!(var, Value::Native(NativeId(1)));
        assert_eq!(var.get_string(&host), "new");
        var.assign(&Value::Int(3), &mut host);
        assert_eq!(var.get_string(&host), "3");
        var.assign(&Value::Bool(true), &mut host);
        assert_eq!(var.get_string(&host), "yes");
        // A float, null or object replaces the native.
        var.assign(&Value::Float(1.0), &mut host);
        assert_eq!(var, Value::Float(1.0));
        let mut var = host.string(2, "old");
        var.assign(&Value::Null, &mut host);
        assert_eq!(var, Value::Null);
        // A plain variable takes a copy.
        let mut var = Value::Int(1);
        let mut obj = Value::object();
        obj.set_property("k", Value::Int(2), &mut host);
        var.assign(&obj, &mut host);
        assert_eq!(var, obj);
        obj.set_property("k", Value::Int(3), &mut host);
        assert_eq!(var.get_property("k", &host), Some(Value::Int(2)));
    }

    #[test]
    fn type_helpers() {
        assert_eq!(Value::Null.type_name(), "null");
        assert_eq!(Value::from(1.0).type_name(), "float");
        assert_eq!(Value::from("s").as_str(), Some("s"));
        assert_eq!(Value::Int(1).as_str(), None);
        assert_eq!(Value::from(NativeId(3)).as_native(), Some(NativeId(3)));
        assert!(Value::Int(1).same_type(&Value::Int(2)));
        assert!(!Value::Int(1).same_type(&Value::Float(1.0)));
        assert_eq!(Value::default(), Value::Null);
        assert_eq!(Value::from(true), Value::Bool(true));
        assert_eq!(Value::from(String::from("x")), Value::from("x"));
    }
}
