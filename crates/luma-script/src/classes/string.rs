//! The `String` class (`CSXString`).
//!
//! Written with reference to `SXString.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use super::{ArrayObject, ClassHost, ClassObject, arg, text_to_utf8, utf8_to_text};
use crate::host::Return;
use crate::value::{Natives, Value};

/// A `String` object: a text and the capacity of the buffer the original
/// engine keeps it in (one more than the text's length, or what the
/// script asked for).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StringObject {
    text: String,
    capacity: i32,
}

/// Clamps a position to `0..=len`.
fn clamp(position: i64, len: i64) -> i64 {
    position.clamp(0, len)
}

impl StringObject {
    /// `new String(value)`: an integer is a capacity and the text is
    /// empty, anything else converts to the text (`new String()` gives
    /// `[null]`, the string form of a missing argument).
    pub fn new(args: &[Value], natives: &dyn Natives) -> StringObject {
        let value = arg(args, 0);
        let mut string = StringObject {
            text: String::new(),
            capacity: 0,
        };
        if let Value::Int(capacity) = value {
            string.capacity = capacity.max(0);
        } else {
            string.set_text(&value.get_string(natives));
        }
        if string.capacity == 0 {
            string.set_text("");
        }
        string
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The buffer capacity (`Capacity`).
    pub fn capacity(&self) -> i32 {
        self.capacity
    }

    fn len(&self) -> i32 {
        self.text.chars().count() as i32
    }

    /// Replaces the text; the capacity grows to hold it and its
    /// terminator.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        let len = self.len();
        if len >= self.capacity {
            self.capacity = len + 1;
        }
    }

    /// `Type`, `Length`, `Capacity`; any other name reads `null`. The
    /// length counts characters: in a UTF-8 game (`utf8`), those the
    /// text's bytes encode.
    pub fn get_property(&self, name: &str, utf8: bool) -> Value {
        match name {
            "Type" => Value::from("string"),
            "Length" if utf8 => Value::Int(utf8_to_text(&self.text).chars().count() as i32),
            "Length" => Value::Int(self.len()),
            "Capacity" => Value::Int(self.capacity),
            _ => Value::Null,
        }
    }

    /// `Capacity` can grow but not shrink below the text (a warning is
    /// logged); any other property is dropped.
    pub fn set_property<H: ClassHost>(&mut self, name: &str, value: &Value, host: &mut H) {
        if name == "Capacity" {
            let capacity = value.get_int(host);
            if capacity < self.len() + 1 {
                host.log("Warning: cannot lower string capacity");
            } else {
                self.capacity = capacity;
            }
        }
    }

    /// The characters from `start` for `count`, both already clamped so
    /// that the slice ends at the text's end at most.
    fn slice(chars: &[char], start: i64, count: i64) -> String {
        let stop = (start + count).min(chars.len() as i64);
        chars[start as usize..stop as usize].iter().collect()
    }

    /// `Substring`, `Substr`, `ToUpperCase`, `ToLowerCase`, `IndexOf`,
    /// `Split`. In a UTF-8 game the methods work on the characters the
    /// text encodes and give UTF-8 text back, as the original engine
    /// does through wide strings.
    pub fn call_method<H: ClassHost>(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut H,
    ) -> Option<Return> {
        let utf8 = host.text_utf8();
        // Text in, as characters; text out, as the game holds it.
        let decode = |text: String| if utf8 { utf8_to_text(&text) } else { text };
        let encode = |text: String| if utf8 { text_to_utf8(&text) } else { text };
        let chars: Vec<char> = decode(self.text.clone()).chars().collect();
        let len = chars.len() as i64;
        Some(match name {
            // Positions are inclusive; they are swapped when reversed and
            // clamped to the text.
            "Substring" => {
                let mut start = i64::from(arg(args, 0).get_int(host));
                let mut end = i64::from(arg(args, 1).get_int(host));
                if end < start {
                    std::mem::swap(&mut start, &mut end);
                }
                let start = clamp(start, len);
                let end = clamp(end, len);
                Return::value(encode(Self::slice(&chars, start, (end - start + 1).max(0))))
            }
            // A missing length means "to the end"; a length of zero or
            // less gives an empty string.
            "Substr" => {
                let start = i64::from(arg(args, 0).get_int(host));
                let length_arg = arg(args, 1);
                let length = i64::from(length_arg.get_int(host));
                if !length_arg.is_null() && length <= 0 {
                    return Some(Return::value(""));
                }
                let start = clamp(start, len);
                let end = if length_arg.is_null() {
                    len
                } else {
                    clamp(start + length - 1, len)
                };
                Return::value(encode(Self::slice(&chars, start, (end - start + 1).max(0))))
            }
            // An ANSI game converts with `_strupr` and `_strlwr` in the C
            // locale the engine runs in: only the letters A to Z change,
            // accented letters stay as they are. A UTF-8 game converts
            // wide characters, every letter that has another case.
            "ToUpperCase" if !utf8 => Return::value(self.text.to_ascii_uppercase()),
            "ToLowerCase" if !utf8 => Return::value(self.text.to_ascii_lowercase()),
            "ToUpperCase" => Return::value(encode(
                chars
                    .iter()
                    .map(|&c| {
                        let mut upper = c.to_uppercase();
                        match (upper.next(), upper.next()) {
                            (Some(u), None) => u,
                            _ => c,
                        }
                    })
                    .collect::<String>(),
            )),
            "ToLowerCase" => Return::value(encode(
                chars
                    .iter()
                    .map(|&c| {
                        let mut lower = c.to_lowercase();
                        match (lower.next(), lower.next()) {
                            (Some(l), None) => l,
                            _ => c,
                        }
                    })
                    .collect::<String>(),
            )),
            // The position of a substring at or after `index`, or -1.
            "IndexOf" => {
                let needle: Vec<char> = decode(arg(args, 0).get_string(host)).chars().collect();
                let index = clamp(i64::from(arg(args, 1).get_int(host)), len) as usize;
                let found = (index..=chars.len())
                    .find(|&i| chars[i..].starts_with(&needle))
                    .map_or(-1, |i| i as i32);
                Return::value(found)
            }
            // Every character of the argument separates; a missing
            // argument means a comma. An empty text gives one empty part.
            // The original looks each character up with `strchr` in the
            // separator string as the game holds it: in a UTF-8 game
            // that compares the low byte of the character with the UTF-8
            // bytes of the separators, so a separator outside ASCII does
            // not split at itself but at the characters whose low byte
            // is one of its bytes, and `Ĭ` (U+012C) splits like a comma.
            "Split" => {
                let separators_arg = arg(args, 0);
                let separators: Vec<char> = if separators_arg.is_null() {
                    vec![',']
                } else {
                    separators_arg.get_string(host).chars().collect()
                };
                let mut parts = Vec::new();
                let mut part = String::new();
                for &c in &chars {
                    let looked_up = if utf8 {
                        char::from(u32::from(c) as u8)
                    } else {
                        c
                    };
                    // A character whose low byte is 0 finds the end of
                    // the separator string, as `strchr` does.
                    if separators.contains(&looked_up) || (utf8 && looked_up == '\0') {
                        parts.push(Value::String(encode(std::mem::take(&mut part))));
                    } else {
                        part.push(c);
                    }
                }
                parts.push(Value::String(encode(part)));
                let array = ArrayObject::from_values(parts);
                let id = host.create_object(ClassObject::Array(array));
                Return::value(Value::Native(id))
            }
            _ => return None,
        })
    }
}
