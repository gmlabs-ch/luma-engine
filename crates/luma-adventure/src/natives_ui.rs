//! The script interface of windows and controls: the properties and
//! methods of `CUIObject`, `CUIWindow`, `CUIButton`, `CUIText` and
//! `CUIEdit` of WME Lite (MIT License, Copyright (c) 2011 Jan Nedoma);
//! see `NOTICE`.

use luma_formats::font::TextAlign;
use luma_formats::{Rgba, VerticalAlign};
use luma_script::{Context, Effect, NativeId, Return, Value};

use crate::ui::{Control, ControlKind, StaticState, WindowMode};
use crate::world::World;

fn native_or_null(id: Option<NativeId>) -> Value {
    id.map_or(Value::Null, Value::Native)
}

fn color_int(c: Rgba) -> i32 {
    luma_script::rgba(
        i32::from(c.r),
        i32::from(c.g),
        i32::from(c.b),
        i32::from(c.a),
    )
}

fn color_of(v: i32) -> Rgba {
    let v = v as u32;
    Rgba {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
        a: ((v >> 24) & 0xff) as u8,
    }
}

impl World {
    /// A built-in property of a control.
    pub(crate) fn control_get(&self, id: NativeId, name: &str) -> Option<Value> {
        let c = self.control(id)?;
        let common = match name {
            "Type" => Some(Value::from(c.type_name())),
            "Name" => Some(Value::from(c.name.clone())),
            "Caption" => Some(Value::from(c.caption.clone())),
            "Parent" => Some(native_or_null(c.parent)),
            "ParentNotify" => Some(Value::Bool(c.parent_notify)),
            "X" => Some(Value::Int(c.x)),
            "Y" => Some(Value::Int(c.y)),
            "Width" => Some(Value::Int(c.width)),
            "Height" => Some(Value::Int(c.height)),
            "Visible" => Some(Value::Bool(c.visible)),
            "Disabled" => Some(Value::Bool(c.disabled)),
            "Text" => Some(Value::from(c.text.clone())),
            "Ready" => Some(Value::Bool(c.window().is_none_or(|w| w.ready))),
            "Filename" => Some(
                c.window()
                    .and_then(|w| w.filename.clone())
                    .map_or(Value::Null, Value::from),
            ),
            "AccCaption" | "Movable" => Some(Value::Null),
            "NextSibling" | "PrevSibling" => {
                let parent = c.parent?;
                let siblings = self.widgets_of(parent);
                let i = siblings.iter().position(|s| *s == id)?;
                let sibling = if name == "NextSibling" {
                    siblings.get(i + 1).copied()
                } else {
                    i.checked_sub(1).and_then(|j| siblings.get(j).copied())
                };
                Some(native_or_null(sibling))
            }
            _ => None,
        };
        if common.is_some() {
            return common;
        }
        Some(match &c.kind {
            ControlKind::Window(w) => match name {
                "NumWidgets" | "NumControls" => Value::Int(w.widgets.len() as i32),
                "Exclusive" => Value::Bool(w.mode == WindowMode::Exclusive),
                "SystemExclusive" => Value::Bool(w.mode == WindowMode::SystemExclusive),
                "Menu" => Value::Bool(w.menu),
                "InGame" => Value::Bool(w.in_game),
                "PauseMusic" => Value::Bool(w.pause_music),
                "ClipContents" => Value::Bool(w.clip_contents),
                "Transparent" => Value::Bool(w.transparent),
                "FadeColor" => Value::Int(w.fade_color.map_or(0, color_int)),
                "AlphaColor" => Value::Int(w.alpha_color.map_or(0, color_int)),
                _ => return None,
            },
            ControlKind::Button(b) => match name {
                "TextAlign" => Value::Int(b.text_align.code()),
                "Focusable" => Value::Bool(c.can_focus),
                "Pressed" => Value::Bool(b.stay_pressed),
                "PixelPerfect" => Value::Bool(b.pixel_perfect),
                _ => return None,
            },
            ControlKind::Static(s) => match name {
                "TextAlign" => Value::Int(s.text_align.code()),
                "VerticalAlign" => Value::Int(s.vertical_align.code()),
                _ => return None,
            },
            ControlKind::Edit(e) => match name {
                "SelStart" => Value::Int(e.sel_start.min(c.text.len()) as i32),
                "SelEnd" => Value::Int(e.sel_end.min(c.text.len()) as i32),
                "FrameWidth" => Value::Int(e.frame_width),
                "MaxLength" => Value::Int(e.max_length),
                "CursorChar" => Value::from(e.cursor_char.clone()),
                "CursorBlinkRate" => Value::Int(e.cursor_blink_rate),
                _ => return None,
            },
        })
    }

    /// Sets a built-in property of a control; `false` when there is no
    /// such property.
    pub(crate) fn control_set(&mut self, id: NativeId, name: &str, value: &Value) -> bool {
        let (text, int, flag) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
        );
        let Some(c) = self.control_mut(id) else {
            return false;
        };
        match name {
            "Name" => c.name = text,
            "Caption" => c.caption = text,
            "ParentNotify" => c.parent_notify = flag,
            "X" => c.x = int,
            "Y" => c.y = int,
            "Width" => c.width = int,
            "Height" => c.height = int,
            "Visible" => c.visible = flag,
            "Disabled" => c.disabled = flag,
            "Text" => c.set_text(&text),
            _ => {
                return match &mut c.kind {
                    ControlKind::Window(w) => match name {
                        "Menu" => {
                            w.menu = flag;
                            true
                        }
                        "InGame" => {
                            w.in_game = flag;
                            true
                        }
                        "PauseMusic" => {
                            w.pause_music = flag;
                            true
                        }
                        "ClipContents" => {
                            w.clip_contents = flag;
                            true
                        }
                        "Transparent" => {
                            w.transparent = flag;
                            true
                        }
                        "FadeColor" => {
                            w.fade_color = (int != 0).then(|| color_of(int));
                            true
                        }
                        "AlphaColor" => {
                            w.alpha_color = (int != 0).then(|| color_of(int));
                            true
                        }
                        _ => false,
                    },
                    ControlKind::Button(b) => match name {
                        "TextAlign" => {
                            b.text_align = TextAlign::from_code(int);
                            true
                        }
                        "Focusable" => {
                            c.can_focus = flag;
                            true
                        }
                        "Pressed" => {
                            b.stay_pressed = flag;
                            true
                        }
                        "PixelPerfect" => {
                            b.pixel_perfect = flag;
                            true
                        }
                        _ => false,
                    },
                    ControlKind::Static(s) => match name {
                        "TextAlign" => {
                            s.text_align = TextAlign::from_code(int);
                            true
                        }
                        "VerticalAlign" => {
                            s.vertical_align = VerticalAlign::from_code(int);
                            true
                        }
                        _ => false,
                    },
                    ControlKind::Edit(e) => match name {
                        "SelStart" => {
                            e.sel_start = int.max(0) as usize;
                            true
                        }
                        "SelEnd" => {
                            e.sel_end = int.max(0) as usize;
                            true
                        }
                        "FrameWidth" => {
                            e.frame_width = int;
                            true
                        }
                        "MaxLength" => {
                            e.max_length = int;
                            true
                        }
                        "CursorChar" => {
                            e.cursor_char =
                                text.chars().next().map(String::from).unwrap_or_default();
                            true
                        }
                        "CursorBlinkRate" => {
                            e.cursor_blink_rate = int;
                            true
                        }
                        _ => false,
                    },
                };
            }
        }
        true
    }

    /// A method call on a control; `None` when it has no such method.
    pub(crate) fn control_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        let kind = match self.control(id).map(|c| &c.kind) {
            Some(ControlKind::Window(_)) => "window",
            Some(ControlKind::Button(_)) => "button",
            Some(ControlKind::Static(_)) => "static",
            Some(ControlKind::Edit(_)) => "editor",
            None => return None,
        };
        // Methods every control has (CUIObject).
        let common = match name {
            "SetFont" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = path.as_deref().is_none_or(|p| self.load_font(p));
                if let Some(c) = self.control_mut(id) {
                    c.font = if ok { path } else { None };
                }
                Some(Return::value(ok))
            }
            "SetImage" => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = path.as_deref().is_none_or(|p| self.load_ui_image(p));
                if let Some(c) = self.control_mut(id) {
                    c.image = if ok { path } else { None };
                    c.anim = None;
                }
                Some(Return::value(ok))
            }
            "GetImage" => Some(Return::value(
                self.control(id)
                    .and_then(|c| c.image.clone())
                    .map_or(Value::Null, Value::from),
            )),
            "GetImageObject" => Some(Return::value(native_or_null(self.sprite_object_id(id)))),
            "Focus" => {
                self.focus_control(id);
                Some(Return::null())
            }
            "MoveAfter" | "MoveBefore" | "MoveToBottom" | "MoveToTop" => {
                let moved = self.move_widget(id, name, &arg(0));
                Some(Return::value(moved))
            }
            _ => None,
        };
        if common.is_some() {
            return common;
        }
        let ret = match (kind, name) {
            ("window", "GetWidget" | "GetControl") => {
                let found = match arg(0) {
                    Value::Int(i) => self.widget_at(id, i),
                    Value::Null => None,
                    v => self.widget(id, &v.get_string(self)),
                };
                Return::value(native_or_null(found))
            }
            ("window", "SetInactiveFont") => {
                let path = arg(0).get_string(self);
                let ok = self.load_font(&path);
                if let Some(w) = self.control_mut(id).and_then(Control::window_mut) {
                    w.font_inactive = ok.then_some(path);
                }
                Return::value(ok)
            }
            ("window", "SetInactiveImage") => {
                let path = arg(0).get_string(self);
                let ok = self.load_ui_image(&path);
                if let Some(w) = self.control_mut(id).and_then(Control::window_mut) {
                    w.image_inactive = ok.then_some(path);
                }
                Return::value(ok)
            }
            ("window", "GetInactiveImage") => Return::value(
                self.control(id)
                    .and_then(Control::window)
                    .and_then(|w| w.image_inactive.clone())
                    .map_or(Value::Null, Value::from),
            ),
            ("window", "GetInactiveImageObject" | "CreateEntityContainer") => {
                self.stub(&format!("window.{name}"));
                Return::null()
            }
            ("window", "Close") => Return::value(self.close_window(ctx.engine, id)),
            ("window", "GoExclusive") => {
                self.go_exclusive(id);
                Return {
                    value: Value::Null,
                    effect: Effect::WaitFor(id),
                }
            }
            ("window", "GoSystemExclusive") => {
                self.go_system_exclusive(ctx.engine, id);
                Return {
                    value: Value::Null,
                    effect: Effect::WaitFor(id),
                }
            }
            ("window", "Center") => {
                self.center_window(id);
                Return::null()
            }
            ("window", "LoadFromFile") => match arg(0) {
                Value::Null => Return::value(true),
                v => {
                    let path = v.get_string(self);
                    Return::value(self.load_window_into(ctx.engine, id, &path))
                }
            },
            ("window", "CreateButton" | "CreateStatic" | "CreateEditor" | "CreateWindow") => {
                let widget_name = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let kind = match name {
                    "CreateButton" => ControlKind::Button(Box::default()),
                    "CreateStatic" => ControlKind::Static(StaticState::default()),
                    "CreateEditor" => ControlKind::Edit(Box::default()),
                    _ => ControlKind::Window(Box::default()),
                };
                let created = self.create_control(id, kind, widget_name.as_deref());
                Return::value(Value::Native(created))
            }
            (
                "window",
                "DeleteControl" | "DeleteButton" | "DeleteStatic" | "DeleteEditor" | "DeleteWindow",
            ) => {
                if let Some(widget) = arg(0).as_native() {
                    if self.control(widget).is_some_and(|c| c.parent == Some(id)) {
                        self.unload_control(ctx.engine, widget);
                    }
                }
                Return::null()
            }
            ("button", "Press") => {
                self.press_button_later(id);
                Return::null()
            }
            (
                "button",
                "SetDisabledFont" | "SetHoverFont" | "SetPressedFont" | "SetFocusedFont",
            ) => {
                let path = match arg(0) {
                    Value::Null => None,
                    v => Some(v.get_string(self)),
                };
                let ok = path.as_deref().is_none_or(|p| self.load_font(p));
                let value = if ok { path } else { None };
                if let Some(b) = self.control_mut(id).and_then(Control::button_mut) {
                    match name {
                        "SetDisabledFont" => b.font_disable = value,
                        "SetHoverFont" => b.font_hover = value,
                        "SetPressedFont" => b.font_press = value,
                        _ => b.font_focus = value,
                    }
                }
                Return::value(ok)
            }
            (
                "button",
                "SetDisabledImage" | "SetHoverImage" | "SetPressedImage" | "SetFocusedImage",
            ) => {
                let path = arg(0).get_string(self);
                let ok = self.load_ui_image(&path);
                let value = ok.then_some(path);
                if let Some(b) = self.control_mut(id).and_then(Control::button_mut) {
                    match name {
                        "SetDisabledImage" => b.image_disable = value,
                        "SetHoverImage" => b.image_hover = value,
                        "SetPressedImage" => b.image_press = value,
                        _ => b.image_focus = value,
                    }
                }
                Return::value(ok)
            }
            (
                "button",
                "GetDisabledImage" | "GetHoverImage" | "GetPressedImage" | "GetFocusedImage",
            ) => {
                let b = self.control(id).and_then(Control::button);
                let path = b.and_then(|b| match name {
                    "GetDisabledImage" => b.image_disable.clone(),
                    "GetHoverImage" => b.image_hover.clone(),
                    "GetPressedImage" => b.image_press.clone(),
                    _ => b.image_focus.clone(),
                });
                Return::value(path.map_or(Value::Null, Value::from))
            }
            (
                "button",
                "GetDisabledImageObject"
                | "GetHoverImageObject"
                | "GetPressedImageObject"
                | "GetFocusedImageObject",
            ) => {
                self.stub(&format!("button.{name}"));
                Return::null()
            }
            ("static", "SizeToFit") => {
                self.size_static_to_fit(id, true);
                Return::null()
            }
            ("static", "HeightToFit") => {
                self.size_static_to_fit(id, false);
                Return::null()
            }
            ("editor", "SetSelectedFont") => {
                let path = arg(0).get_string(self);
                let ok = self.load_font(&path);
                if let Some(e) = self.control_mut(id).and_then(Control::edit_mut) {
                    e.font_selected = ok.then_some(path);
                }
                Return::value(ok)
            }
            _ => return None,
        };
        Some(ret)
    }

    /// Reorders a control among its siblings (`MoveAfter`, `MoveBefore`,
    /// `MoveToBottom`, `MoveToTop`).
    fn move_widget(&mut self, id: NativeId, how: &str, other: &Value) -> bool {
        let Some(parent) = self.control(id).and_then(|c| c.parent) else {
            return false;
        };
        let siblings = self.widgets_of(parent);
        let Some(from) = siblings.iter().position(|s| *s == id) else {
            return false;
        };
        let target = match how {
            "MoveToBottom" => Some(0),
            "MoveToTop" => Some(siblings.len() - 1),
            _ => {
                let found = match other {
                    Value::Native(n) => siblings.iter().position(|s| s == n),
                    v => {
                        let name = v.get_string(self);
                        siblings.iter().position(|s| {
                            self.control(*s)
                                .is_some_and(|c| c.name.eq_ignore_ascii_case(&name))
                        })
                    }
                };
                found.map(|i| if how == "MoveAfter" { i + 1 } else { i })
            }
        };
        let Some(mut to) = target else {
            return false;
        };
        let Some(w) = self.control_mut(parent).and_then(Control::window_mut) else {
            return false;
        };
        let moved = w.widgets.remove(from);
        if to > from {
            to -= 1;
        }
        w.widgets.insert(to.min(w.widgets.len()), moved);
        true
    }
}
