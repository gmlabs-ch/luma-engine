//! The `MemBuffer` class (`CSXMemBuffer`).
//!
//! Written with reference to `SXMemBuffer.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::cmp::Ordering;

use super::{ClassHost, arg, text_from_bytes, text_to_bytes};
use crate::host::{Context, Return};
use crate::value::{Natives, Value};

/// A `MemBuffer` object: a block of bytes, little-endian like the
/// machine the original engine ran on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MemBuffer {
    data: Vec<u8>,
}

impl MemBuffer {
    /// `new MemBuffer(size)`: `size` zero bytes.
    pub fn new(args: &[Value], natives: &dyn Natives) -> MemBuffer {
        let size = arg(args, 0).get_int(natives).max(0) as usize;
        MemBuffer {
            data: vec![0; size],
        }
    }

    /// The bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Orders two buffers (`ScCompare`): equal only when they are the
    /// same object, "greater" otherwise.
    pub fn compare(handles: Ordering) -> Ordering {
        if handles == Ordering::Equal {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }

    /// `Type` and `Size`; other names are script properties.
    pub fn get_property(&self, name: &str) -> Option<Value> {
        match name {
            "Type" => Some(Value::from("membuffer")),
            "Size" => Some(Value::Int(self.data.len() as i32)),
            _ => None,
        }
    }

    /// Whether `len` bytes at `start` are inside the buffer; otherwise
    /// a runtime error is logged (`CheckBounds`). An empty buffer has no
    /// room for anything, where the original reads past its
    /// zero-length allocation.
    fn check<H: ClassHost>(&self, host: &mut H, ctx: &Context<'_>, start: i64, len: i64) -> bool {
        let size = self.data.len() as i64;
        if start < 0 || len <= 0 || start + len > size {
            host.script_error(&ctx.runtime_error("Set/Get method call is out of bounds"));
            false
        } else {
            true
        }
    }

    fn range(&self, start: i64, len: i64) -> std::ops::Range<usize> {
        start as usize..(start + len) as usize
    }

    /// `SetSize`, the typed getters and setters, `GetString`,
    /// `SetString`; `GetPointer`, `SetPointer` (raw addresses, which have
    /// no meaning here: a runtime error, `null` or `false`) and
    /// `DEBUG_Dump` (nothing).
    pub fn call_method<H: ClassHost>(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut H,
        ctx: &Context<'_>,
    ) -> Option<Return> {
        let start = {
            let natives: &dyn Natives = host;
            i64::from(arg(args, 0).get_int(natives))
        };
        let value = arg(args, 1);
        macro_rules! get {
            ($len:expr, $read:expr) => {{
                if self.check(host, ctx, start, $len) {
                    let bytes = &self.data[self.range(start, $len)];
                    Return::value($read(bytes))
                } else {
                    Return::null()
                }
            }};
        }
        macro_rules! set {
            ($bytes:expr) => {{
                let bytes: Vec<u8> = $bytes;
                if self.check(host, ctx, start, bytes.len() as i64) {
                    let range = self.range(start, bytes.len() as i64);
                    self.data[range].copy_from_slice(&bytes);
                    Return::value(true)
                } else {
                    Return::value(false)
                }
            }};
        }
        Some(match name {
            "SetSize" => {
                let size = value_int(&arg(args, 0), host).max(0) as usize;
                self.data.resize(size, 0);
                Return::value(true)
            }
            "GetBool" => get!(1, |b: &[u8]| Value::Bool(b[0] != 0)),
            "GetByte" => get!(1, |b: &[u8]| Value::Int(i32::from(b[0]))),
            // The original adds 65536 to the signed value it read.
            "GetShort" => get!(2, |b: &[u8]| Value::Int(
                65_536 + i32::from(i16::from_le_bytes([b[0], b[1]]))
            )),
            "GetInt" | "GetLong" => get!(4, |b: &[u8]| Value::Int(i32::from_le_bytes([
                b[0], b[1], b[2], b[3]
            ]))),
            "GetFloat" => get!(4, |b: &[u8]| Value::Float(f64::from(f32::from_le_bytes([
                b[0], b[1], b[2], b[3]
            ])))),
            "GetDouble" => get!(8, |b: &[u8]| Value::Float(f64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]
            ]))),
            // A length of zero reads up to the terminating zero; the
            // text stops at a zero either way.
            "GetString" => {
                let mut length = i64::from(value_int(&value, host));
                let size = self.data.len() as i64;
                if length == 0 && start >= 0 && start < size {
                    if let Some(i) = self.data[start as usize..].iter().position(|&b| b == 0) {
                        length = i as i64;
                    }
                }
                if self.check(host, ctx, start, length) {
                    let bytes = &self.data[self.range(start, length)];
                    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                    Return::value(text_from_bytes(&bytes[..end]))
                } else {
                    Return::null()
                }
            }
            "GetPointer" => {
                host.script_error(
                    &ctx.runtime_error("MemBuffer.GetPointer: memory pointers are not available"),
                );
                Return::null()
            }
            "SetBool" => {
                let v = value_bool(&value, host);
                set!(vec![u8::from(v)])
            }
            "SetByte" => {
                let v = value_int(&value, host);
                set!(vec![v as u8])
            }
            "SetShort" => {
                let v = value_int(&value, host);
                set!((v as i16).to_le_bytes().to_vec())
            }
            "SetInt" | "SetLong" => {
                let v = value_int(&value, host);
                set!(v.to_le_bytes().to_vec())
            }
            "SetFloat" => {
                let v = value_float(&value, host);
                set!((v as f32).to_le_bytes().to_vec())
            }
            "SetDouble" => {
                let v = value_float(&value, host);
                set!(v.to_le_bytes().to_vec())
            }
            // The terminating zero is written too.
            "SetString" => {
                let natives: &dyn Natives = host;
                let mut bytes = text_to_bytes(&value.get_string(natives));
                bytes.push(0);
                set!(bytes)
            }
            "SetPointer" => {
                host.script_error(
                    &ctx.runtime_error("MemBuffer.SetPointer: memory pointers are not available"),
                );
                Return::value(false)
            }
            "DEBUG_Dump" => Return::null(),
            _ => return None,
        })
    }
}

fn value_int<H: ClassHost>(value: &Value, host: &H) -> i32 {
    let natives: &dyn Natives = host;
    value.get_int(natives)
}

fn value_bool<H: ClassHost>(value: &Value, host: &H) -> bool {
    let natives: &dyn Natives = host;
    value.get_bool(natives)
}

fn value_float<H: ClassHost>(value: &Value, host: &H) -> f64 {
    let natives: &dyn Natives = host;
    value.get_float(natives)
}
