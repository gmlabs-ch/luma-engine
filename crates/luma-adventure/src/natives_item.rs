//! The script interface of inventory items (`CAdItem` of WME Lite, MIT
//! License, Copyright (c) 2011 Jan Nedoma; see `NOTICE`).

use luma_formats::font::TextAlign;
use luma_script::{NativeId, Return, Value};

use crate::world::World;

impl World {
    /// A built-in property of an item.
    pub(crate) fn item_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let item = self.item(id)?;
        Some(match name {
            "Type" => Value::from("item"),
            "Name" => Value::from(item.def.name.clone()),
            "Caption" => Value::from(item.def.caption.clone()),
            "X" => Value::Int(item.x),
            "Y" => Value::Int(item.y),
            "Height" => Value::Int(0),
            "Ready" | "Active" | "Interactive" | "Registrable" | "Movable" => Value::Bool(true),
            "Filename" => Value::Null,
            "DisplayAmount" => Value::Bool(item.def.display_amount),
            "Amount" => Value::Int(item.def.amount),
            "AmountOffsetX" => Value::Int(item.def.amount_offset_x),
            "AmountOffsetY" => Value::Int(item.def.amount_offset_y),
            "AmountAlign" => Value::Int(item.def.amount_align.code()),
            "AmountString" => item
                .def
                .amount_string
                .clone()
                .map_or(Value::Null, Value::from),
            "CursorCombined" => Value::Bool(item.def.cursor_combined),
            _ => return None,
        })
    }

    /// Sets a built-in property of an item; `false` when there is no
    /// such property.
    pub(crate) fn item_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (text, int, flag, is_null) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
            value.is_null(),
        );
        let caption = self.expand(&text);
        let Some(item) = self.item_mut(id) else {
            return false;
        };
        match name {
            "Name" => item.def.name = text,
            "Caption" => item.def.caption = caption,
            "DisplayAmount" => item.def.display_amount = flag,
            "Amount" => item.def.amount = int,
            "AmountOffsetX" => item.def.amount_offset_x = int,
            "AmountOffsetY" => item.def.amount_offset_y = int,
            "AmountAlign" => item.def.amount_align = TextAlign::from_code(int),
            "AmountString" => item.def.amount_string = (!is_null).then_some(text),
            "CursorCombined" => item.def.cursor_combined = flag,
            _ => return false,
        }
        true
    }

    /// A method call on an item; `None` when it has no such method.
    pub(crate) fn item_call(&mut self, id: NativeId, name: &str, args: &[Value]) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        self.item(id)?;
        let ret = match name {
            "SetSprite" | "SetHoverSprite" => {
                let path = arg(0).get_string(self);
                let ok = self.load_ui_image(&path);
                if let Some(item) = self.item_mut(id) {
                    let slot = if name == "SetSprite" {
                        &mut item.def.sprite
                    } else {
                        &mut item.def.sprite_hover
                    };
                    *slot = ok.then_some(path);
                    item.anim = None;
                }
                Return::value(ok)
            }
            "GetSprite" | "GetHoverSprite" => {
                let item = self.item(id)?;
                let path = if name == "GetSprite" {
                    item.def.sprite.clone()
                } else {
                    item.def.sprite_hover.clone()
                };
                Return::value(path.map_or(Value::Null, Value::from))
            }
            "SetNormalCursor" | "SetHoverCursor" => {
                let path = arg(0).get_string(self);
                let ok = self.load_cursor(&path);
                if let Some(item) = self.item_mut(id) {
                    let slot = if name == "SetNormalCursor" {
                        &mut item.def.cursor
                    } else {
                        &mut item.def.cursor_hover
                    };
                    *slot = ok.then_some(path);
                }
                Return::value(ok)
            }
            "GetNormalCursor" | "GetHoverCursor" => {
                let item = self.item(id)?;
                let path = if name == "GetNormalCursor" {
                    item.def.cursor.clone()
                } else {
                    item.def.cursor_hover.clone()
                };
                Return::value(path.map_or(Value::Null, Value::from))
            }
            "SetCaption" => {
                let caption = arg(0).get_string(self);
                let caption = self.expand(&caption);
                if let Some(item) = self.item_mut(id) {
                    item.def.caption = caption;
                }
                Return::null()
            }
            "SetFont" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = path.as_deref().is_none_or(|p| self.load_font(p));
                if let Some(item) = self.item_mut(id) {
                    item.def.font = if ok { path } else { None };
                }
                Return::value(ok)
            }
            "GetFont" => Return::value(
                self.item(id)
                    .and_then(|i| i.def.font.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "IsTalking" => Return::value(false),
            "GetSpriteObject"
            | "GetHoverSpriteObject"
            | "GetNormalCursorObject"
            | "GetHoverCursorObject"
            | "Talk"
            | "TalkAsync"
            | "StopTalk"
            | "PlayAnim"
            | "PlayAnimAsync"
            | "Reset"
            | "ForceTalkAnim"
            | "AddTalkSprite"
            | "RemoveTalkSprite"
            | "SetTalkSprite"
            | "SetCursor"
            | "RemoveCursor" => {
                self.stub(&format!("Item.{name}"));
                Return::null()
            }
            _ => return None,
        };
        Some(ret)
    }
}
