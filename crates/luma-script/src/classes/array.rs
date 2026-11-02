//! The `Array` class (`CSXArray`).
//!
//! Written with reference to `SXArray.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::collections::BTreeMap;

use crate::host::Return;
use crate::value::{Natives, Value, atoi};

/// An `Array` object: a length and the elements that were set, by index.
/// An element never set reads `null` and is left out of the string form,
/// where an element set to `null` prints `[null]`, as in the original
/// engine, which keeps the elements as properties named by their index.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArrayObject {
    length: i32,
    values: BTreeMap<i32, Value>,
}

impl ArrayObject {
    /// `new Array()`: empty; `new Array(n)`: `n` unset elements (any
    /// single argument is a length, converted to an integer);
    /// `new Array(a, b, ...)`: the elements.
    pub fn new(args: &[Value], natives: &dyn Natives) -> ArrayObject {
        match args {
            [] => ArrayObject::default(),
            [length] => ArrayObject {
                length: length.get_int(natives).max(0),
                values: BTreeMap::new(),
            },
            _ => ArrayObject::from_values(args.to_vec()),
        }
    }

    /// An array holding these elements.
    pub fn from_values(values: Vec<Value>) -> ArrayObject {
        ArrayObject {
            length: values.len() as i32,
            values: values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i as i32, v))
                .collect(),
        }
    }

    /// The length (`Length`).
    pub fn len(&self) -> i32 {
        self.length
    }

    /// Whether the length is zero.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// The element at `index`, if one was set.
    pub fn get(&self, index: i32) -> Option<&Value> {
        self.values.get(&index)
    }

    /// Appends an element and returns the new length.
    pub fn push(&mut self, value: Value) -> i32 {
        self.values.insert(self.length, value);
        self.length += 1;
        self.length
    }

    /// Removes and returns the last element: `null` when the array is
    /// empty or the element was never set.
    pub fn pop(&mut self) -> Value {
        if self.length <= 0 {
            return Value::Null;
        }
        self.length -= 1;
        self.values.remove(&self.length).unwrap_or(Value::Null)
    }

    /// Sets the length; the elements past it are dropped.
    pub fn set_len(&mut self, length: i32) {
        self.length = length.max(0);
        let length = self.length;
        self.values.retain(|&i, _| i < length);
    }

    /// Sets an element; the array grows to include it.
    pub fn set(&mut self, index: i32, value: Value) {
        if index < 0 {
            return;
        }
        if index >= self.length {
            self.length = index + 1;
        }
        self.values.insert(index, value);
    }

    /// An index written as a property name: all digits (`ValidNumber`),
    /// so `"007"` is 7 and `"1.000000"` or `"-1"` is no index.
    fn index(name: &str) -> Option<i32> {
        if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
            Some(atoi(name))
        } else {
            None
        }
    }

    /// The elements joined by commas (`ScToString`).
    pub fn to_string(&self, natives: &dyn Natives) -> String {
        let mut out = String::new();
        for i in 0..self.length {
            if let Some(value) = self.values.get(&i) {
                out.push_str(&value.get_string(natives));
            }
            if i < self.length - 1 {
                out.push(',');
            }
        }
        out
    }

    /// `Type`, `Length`, `[index]`; any other name reads `null`.
    pub fn get_property(&self, name: &str) -> Value {
        match name {
            "Type" => Value::from("array"),
            "Length" => Value::Int(self.length),
            _ => match Self::index(name) {
                Some(index) => self.values.get(&index).cloned().unwrap_or(Value::Null),
                None => Value::Null,
            },
        }
    }

    /// `Length` and `[index]`; any other property is dropped.
    pub fn set_property(&mut self, name: &str, value: &Value, natives: &dyn Natives) {
        if name == "Length" {
            self.set_len(value.get_int(natives));
        } else if let Some(index) = Self::index(name) {
            self.set(index, value.clone());
        }
    }

    /// `Push(...)` and `Pop()`.
    pub fn call_method(
        &mut self,
        name: &str,
        args: &[Value],
        _natives: &dyn Natives,
    ) -> Option<Return> {
        Some(match name {
            "Push" => {
                for value in args {
                    self.push(value.clone());
                }
                Return::value(self.length)
            }
            "Pop" => Return::value(self.pop()),
            _ => return None,
        })
    }
}
