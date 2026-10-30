//! Dialogue responses (`CAdResponseBox`, `CAdResponse`): the choices a
//! script offers with `Game.AddResponse`, listed in the response box
//! when `Game.GetResponse` waits for the player's pick.
//!
//! The rules follow the original engine's `AdGame.cpp` and
//! `AdResponseBox.cpp` (Wintermute Engine 1.x, MIT): a choice added
//! "once" is not offered again within the current dialogue branch
//! (`StartDlgBranch`/`EndDlgBranch`), one added "once per game" never
//! again; `ResetResponse` forgets either.

use luma_formats::{ResponseBoxDef, Rgba, VerticalAlign};
use luma_render::{DrawList, Texture};
use luma_script::{Effect, Engine, NativeId, Return, ScriptId, Value};

use crate::input::HitArea;
use crate::ui::{ButtonListener, ButtonState, Control, ControlKind};
use crate::world::World;

/// How often a response may be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResponseKind {
    /// Every time it is added (`AddResponse`).
    Normal,
    /// Once per dialogue branch (`AddResponseOnce`).
    Once,
    /// Once per game (`AddResponseOnceGame`).
    OnceGame,
}

/// A choice offered to the player.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Response {
    /// The value `GetResponse` returns when it is chosen.
    pub id: i32,
    /// The text shown, through the string table.
    pub text: String,
    /// The text as the script gave it.
    pub text_orig: String,
    /// An image instead of the text, and its hover and pressed states.
    pub icon: Option<String>,
    /// See `icon`.
    pub icon_hover: Option<String>,
    /// See `icon`.
    pub icon_pressed: Option<String>,
    /// A font of its own.
    pub font: Option<String>,
    /// How often it may be chosen.
    pub kind: ResponseKind,
}

/// The response box and the dialogue state.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Responses {
    /// The box's definition, when the game has one.
    pub def: Option<ResponseBoxDef>,
    /// The box's window.
    pub window: Option<NativeId>,
    /// The choices added since the last `GetResponse`.
    pub pending: Vec<Response>,
    /// The buttons showing the pending choices while the box waits.
    pub buttons: Vec<NativeId>,
    /// The script waiting in `GetResponse`.
    pub waiting: Option<ScriptId>,
    /// The first choice shown (`prev` and `next`).
    pub scroll_offset: usize,
    /// The dialogue branches open, innermost last.
    pub branches: Vec<String>,
    /// The "once" responses chosen, with the branch they were chosen in.
    pub used_branch: Vec<(Option<String>, i32)>,
    /// The "once per game" responses chosen.
    pub used_game: Vec<i32>,
    /// The text of the last response chosen (`Game.LastResponse`).
    pub last_text: Option<String>,
    /// See `last_text`, as the script gave it (`LastResponseOrig`).
    pub last_text_orig: Option<String>,
}

impl World {
    /// Loads the response box the game file names (`RESPONSE_BOX`).
    pub(crate) fn load_response_box(&mut self, engine: &mut Engine) {
        if let Some(path) = self.game_file().response_box.clone() {
            self.load_response_box_from(engine, &path);
        }
    }

    /// Loads a response box definition and creates its window
    /// (`Game.LoadResponseBox`).
    pub(crate) fn load_response_box_from(&mut self, engine: &mut Engine, path: &str) -> bool {
        let bytes = match self.vfs.read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!("Error opening response box '{path}': {err}"));
                return false;
            }
        };
        let mut def = match ResponseBoxDef::parse(&bytes) {
            Ok(def) => def,
            Err(err) => {
                self.log(format!("Error parsing response box '{path}': {err}"));
                return false;
            }
        };
        self.clear_responses(engine);
        if let Some(window_def) = def.window.take() {
            let window = self.new_window_control(None);
            self.apply_window_def(engine, window, window_def, None);
            for widget in self.widgets_of(window) {
                if let Some(b) = self.control_mut(widget).and_then(Control::button_mut) {
                    b.listener = Some(ButtonListener::ResponseBox);
                }
            }
            if let Some(control) = self.control_mut(window) {
                control.visible = false;
            }
            self.responses.window = Some(window);
        }
        for font in [&def.font, &def.font_hover].into_iter().flatten() {
            self.load_font(font);
        }
        self.responses.def = Some(def);
        true
    }

    /// Adds a choice (`AddResponse`, `AddResponseOnce`,
    /// `AddResponseOnceGame`).
    #[allow(clippy::too_many_arguments, reason = "the engine's own signature")]
    pub(crate) fn add_response(
        &mut self,
        kind: ResponseKind,
        id: i32,
        text: &str,
        icon: Option<String>,
        icon_hover: Option<String>,
        icon_pressed: Option<String>,
        font: Option<String>,
    ) {
        let expanded = self.expand(text);
        self.responses.pending.push(Response {
            id,
            text: expanded,
            text_orig: text.to_string(),
            icon,
            icon_hover,
            icon_pressed,
            font,
            kind,
        });
    }

    /// Whether a "once" response was chosen in the current branch.
    fn branch_response_used(&self, id: i32) -> bool {
        let context = self.responses.branches.last().cloned();
        self.responses
            .used_branch
            .iter()
            .any(|(ctx, used)| *used == id && *ctx == context)
    }

    /// Drops the pending choices already chosen where they may not come
    /// back (`WeedResponses`).
    fn weed_responses(&mut self) {
        let used_branch: Vec<i32> = self
            .responses
            .pending
            .iter()
            .filter(|r| r.kind == ResponseKind::Once && self.branch_response_used(r.id))
            .map(|r| r.id)
            .collect();
        let used_game = self.responses.used_game.clone();
        self.responses.pending.retain(|r| match r.kind {
            ResponseKind::Normal => true,
            ResponseKind::Once => !used_branch.contains(&r.id),
            ResponseKind::OnceGame => !used_game.contains(&r.id),
        });
    }

    /// Forgets that a response was chosen (`ResetResponse`).
    pub(crate) fn reset_response(&mut self, id: i32) {
        self.responses.used_branch.retain(|(_, used)| *used != id);
        self.responses.used_game.retain(|used| *used != id);
    }

    /// Drops the pending choices and their buttons (`ClearResponses`).
    pub(crate) fn clear_responses(&mut self, engine: &mut Engine) {
        self.responses.pending.clear();
        self.clear_response_buttons(engine);
    }

    fn clear_response_buttons(&mut self, engine: &mut Engine) {
        let buttons = std::mem::take(&mut self.responses.buttons);
        for id in buttons {
            for script in engine.scripts_of(id) {
                engine.finish(script, true);
            }
            if let Some(window) = self
                .responses
                .window
                .and_then(|w| self.control_mut(w))
                .and_then(Control::window_mut)
            {
                window.widgets.retain(|w| *w != id);
            }
            self.ui.controls.remove(&id);
            self.objects.remove(id);
        }
    }

    /// The number of choices the box would show (`GetNumResponses`).
    pub(crate) fn num_responses(&mut self) -> usize {
        self.weed_responses();
        self.responses.pending.len()
    }

    /// Shows the pending choices and makes the calling script wait for
    /// the player's pick (`GetResponse`): `null` at once without any,
    /// the only one at once when `auto_select_last` asks for it.
    pub(crate) fn get_response(
        &mut self,
        engine: &mut Engine,
        script: ScriptId,
        auto_select_last: bool,
    ) -> Return {
        self.weed_responses();
        if self.responses.pending.is_empty() {
            return Return::null();
        }
        if self.responses.pending.len() == 1 && auto_select_last {
            let id = self.responses.pending[0].id;
            self.handle_response(0);
            self.clear_responses(engine);
            return Return::value(id);
        }
        let Some(window) = self.responses.window else {
            self.log("Game.GetResponse: response box is not defined".to_string());
            return Return::null();
        };
        self.create_response_buttons();
        self.responses.waiting = Some(script);
        self.go_exclusive(window);
        self.layout_responses();
        Return {
            value: Value::Null,
            effect: Effect::WaitForExclusive(self.response_box_id),
        }
    }

    /// One button per pending choice, in the box's window
    /// (`CreateButtons`): the text in the box's fonts, or the icons.
    fn create_response_buttons(&mut self) {
        let Some(window) = self.responses.window else {
            return;
        };
        let Some(def) = self.responses.def.clone() else {
            return;
        };
        self.responses.scroll_offset = 0;
        let pending = self.responses.pending.clone();
        let area_width = def.area.width() as i32;
        let screen_width = self.settings.width as i32;
        for (index, response) in pending.iter().enumerate() {
            let hover = def.font_hover.clone();
            let state = ButtonState {
                listener: Some(ButtonListener::Response(index)),
                text_align: def.text_align,
                font_hover: hover.clone(),
                font_press: hover,
                ..ButtonState::default()
            };
            let id = self.create_control(
                window,
                ControlKind::Button(Box::new(state)),
                Some("response"),
            );
            if let Some(icon) = &response.icon {
                self.load_ui_image(icon);
                for extra in [&response.icon_hover, &response.icon_pressed]
                    .into_iter()
                    .flatten()
                {
                    self.load_ui_image(extra);
                }
                if let Some(control) = self.control_mut(id) {
                    control.image = Some(icon.clone());
                    control.caption = response.text.clone();
                    if let Some(b) = control.button_mut() {
                        b.image_hover = response.icon_hover.clone();
                        b.image_press = response.icon_pressed.clone();
                    }
                }
            } else {
                let font = response.font.clone().or_else(|| def.font.clone());
                if let Some(font) = &font {
                    self.load_font(font);
                }
                if let Some(control) = self.control_mut(id) {
                    control.text = response.text.clone();
                    control.font = font;
                    control.width = if area_width > 0 {
                        area_width
                    } else {
                        screen_width
                    };
                }
            }
            if let Some(control) = self.control_mut(id) {
                control.can_focus = false;
                control.visible = false;
                control.cursor = def.cursor.clone();
            }
            self.correct_size(id);
            self.responses.buttons.push(id);
        }
    }

    /// Places the buttons in the area for this frame (`Display`): one
    /// under the other (or side by side), from the scroll offset, as
    /// many as fit; `prev` and `next` show when there is more.
    pub(crate) fn layout_responses(&mut self) {
        if self.responses.waiting.is_none() {
            return;
        }
        let (Some(window), Some(def)) = (self.responses.window, self.responses.def.clone()) else {
            return;
        };
        let buttons = self.responses.buttons.clone();
        let sizes: Vec<(i32, i32)> = buttons
            .iter()
            .map(|b| self.control(*b).map_or((0, 0), |c| (c.width, c.height)))
            .collect();
        let (left, top, right, bottom) =
            (def.area.left, def.area.top, def.area.right, def.area.bottom);
        let (mut x, mut y) = (left, top);
        if !def.horizontal {
            let total: i32 = sizes.iter().map(|(_, h)| h + def.spacing).sum::<i32>() - def.spacing;
            match def.vertical_align {
                VerticalAlign::Bottom => {
                    if y + total < bottom {
                        y = bottom - total;
                    }
                }
                VerticalAlign::Center => {
                    if y + total < bottom {
                        y += ((bottom - top) - total) / 2;
                    }
                }
                VerticalAlign::Top => {}
            }
        }
        let offset = self.responses.scroll_offset.min(buttons.len());
        let mut scroll_needed = false;
        let mut placed = vec![None; buttons.len()];
        for (i, (w, h)) in sizes.iter().enumerate().skip(offset) {
            if (def.horizontal && x + w > right) || (!def.horizontal && y + h > bottom) {
                scroll_needed = true;
                break;
            }
            placed[i] = Some((x, y));
            if def.horizontal {
                x += w + def.spacing;
            } else {
                y += h + def.spacing;
            }
        }
        for (i, id) in buttons.iter().enumerate() {
            if let Some(control) = self.control_mut(*id) {
                match placed[i] {
                    Some((px, py)) => {
                        control.visible = true;
                        control.x = px;
                        control.y = py;
                    }
                    None => control.visible = false,
                }
            }
        }
        let show_prev = offset > 0;
        for (name, shown) in [("prev", show_prev), ("next", scroll_needed)] {
            if let Some(button) = self.widget(window, name) {
                if let Some(control) = self.control_mut(button) {
                    control.visible = shown;
                }
            }
        }
    }

    /// Notes a chosen response (`HandleResponse`).
    fn handle_response(&mut self, index: usize) {
        let Some(response) = self.responses.pending.get(index).cloned() else {
            return;
        };
        self.responses.last_text = Some(response.text.clone());
        self.responses.last_text_orig = Some(response.text_orig.clone());
        match response.kind {
            ResponseKind::Normal => {}
            ResponseKind::Once => {
                if !self.branch_response_used(response.id) {
                    let context = self.responses.branches.last().cloned();
                    self.responses.used_branch.push((context, response.id));
                }
            }
            ResponseKind::OnceGame => {
                if !self.responses.used_game.contains(&response.id) {
                    self.responses.used_game.push(response.id);
                }
            }
        }
    }

    /// The player picked a choice: the waiting script gets its id and
    /// goes on, the box closes.
    pub(crate) fn select_response(&mut self, engine: &mut Engine, index: usize) {
        let Some(id) = self.responses.pending.get(index).map(|r| r.id) else {
            return;
        };
        self.handle_response(index);
        if let Some(script) = self.responses.waiting.take() {
            engine.set_result(script, Value::Int(id));
        }
        if let Some(window) = self.responses.window {
            self.close_window(engine, window);
        }
        self.clear_responses(engine);
    }

    /// The box's `prev` and `next` buttons.
    pub(crate) fn responses_listen(&mut self, button: &str) {
        if button.eq_ignore_ascii_case("prev") {
            self.responses.scroll_offset = self.responses.scroll_offset.saturating_sub(1);
        } else if button.eq_ignore_ascii_case("next") {
            self.responses.scroll_offset += 1;
        }
    }

    /// Opens a dialogue branch (`StartDlgBranch`): named by the script,
    /// else after its line, and qualified by the script file and event.
    pub(crate) fn start_dlg_branch(
        &mut self,
        engine: &Engine,
        script: ScriptId,
        line: u32,
        name: Option<String>,
    ) {
        let name = name.unwrap_or_else(|| format!("line{line}"));
        self.responses
            .branches
            .push(self.qualified_branch(engine, script, &name));
    }

    fn qualified_branch(&self, engine: &Engine, script: ScriptId, name: &str) -> String {
        let (file, event) = engine
            .script(script)
            .map_or((String::new(), String::new()), |s| {
                (
                    s.source().to_string(),
                    s.thread_event().unwrap_or_default().to_string(),
                )
            });
        format!("{name}.{file}.{event}")
    }

    /// Closes a dialogue branch and those opened inside it
    /// (`EndDlgBranch`); the innermost without a name. When no branch
    /// is left the "once" responses are forgotten.
    pub(crate) fn end_dlg_branch(
        &mut self,
        engine: &Engine,
        script: ScriptId,
        name: Option<String>,
    ) {
        let full = match name {
            Some(name) => self.qualified_branch(engine, script, &name),
            None => match self.responses.branches.last() {
                Some(last) => last.clone(),
                None => return,
            },
        };
        if let Some(start) = self
            .responses
            .branches
            .iter()
            .rposition(|b| b.eq_ignore_ascii_case(&full))
        {
            self.responses.branches.truncate(start);
        }
        if self.responses.branches.is_empty() {
            self.responses.used_branch.clear();
        }
    }

    /// The innermost dialogue branch (`GetCurrentDlgBranch`).
    pub(crate) fn current_dlg_branch(&self) -> Option<String> {
        self.responses.branches.last().cloned()
    }

    /// The texts of the choices the box shows, in order (for listings).
    pub fn response_texts(&self) -> Vec<String> {
        if !self.responses_visible() {
            return Vec::new();
        }
        self.responses
            .pending
            .iter()
            .map(|r| r.text.clone())
            .collect()
    }

    /// Whether the box waits for a pick (`Game.ResponsesVisible`).
    pub fn responses_visible(&self) -> bool {
        self.responses.waiting.is_some()
    }

    /// The response box's window, when the game has one.
    pub fn responses_window(&self) -> Option<NativeId> {
        self.responses.window
    }

    /// Draws the box over everything while it waits.
    pub(crate) fn draw_responses<'a>(
        &'a self,
        list: &mut DrawList<'a>,
        white: Option<&'a Texture>,
    ) {
        if let (true, Some(window)) = (self.responses_visible(), self.responses.window) {
            self.draw_window(list, window, 0, 0, white, Rgba::OPAQUE_WHITE);
        }
    }

    /// Registers where the box catches the mouse while it waits.
    pub(crate) fn responses_hits(&mut self, hits: &mut Vec<HitArea>) {
        if let (true, Some(window)) = (self.responses_visible(), self.responses.window) {
            self.window_hits(window, 0, 0, hits);
        }
    }
}
