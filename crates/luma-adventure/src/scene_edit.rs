//! Scene content scripts create and remove at run time: entities and
//! regions added to a layer, free entities created or loaded into the
//! scene, layers added and deleted, and the region a free object sticks
//! to when the scene is drawn.
//!
//! Handles to layers and nodes are positional (`Kind::Layer`,
//! `Kind::Region`, `EntityRef::Node`, `EntityRef::Free`), so inserting
//! or deleting one renumbers the handles and the states of those after
//! it, in the object table, the scene view and the actors.
//!
//! Written with reference to `AdScene.cpp`, `AdLayer.cpp`, `AdGame.cpp`
//! and `AdObject.cpp` of Wintermute Engine 1.x (MIT License, Copyright
//! (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_formats::scene::{Entity, Layer, Region, SceneNode};
use luma_script::{Engine, NativeId, Value};

use crate::objects::Kind;
use crate::scene_view::EntityRef;
use crate::world::World;

impl World {
    /// Renumbers every handle and state naming a layer node, a layer or
    /// a free entity, after an insertion or a deletion: `node` maps a
    /// `(layer, index)` pair, `layer` a layer index and `free` a free
    /// entity's index to their new values.
    fn remap_refs(
        &mut self,
        node: impl Fn(usize, usize) -> (usize, usize),
        layer: impl Fn(usize) -> usize,
        free: impl Fn(usize) -> usize,
    ) {
        let map_ref = |r: EntityRef| match r {
            EntityRef::Node { layer: l, index } => {
                let (l, index) = node(l, index);
                EntityRef::Node { layer: l, index }
            }
            EntityRef::Free(i) => EntityRef::Free(free(i)),
            other => other,
        };
        self.objects.remap_kinds(|kind| match kind {
            Kind::Entity(r) => Kind::Entity(map_ref(*r)),
            Kind::Region { layer: l, index } => {
                let (l, index) = node(*l, *index);
                Kind::Region { layer: l, index }
            }
            Kind::Layer(i) => Kind::Layer(layer(*i)),
            other => other.clone(),
        });
        if let Some(view) = self.scene.as_mut() {
            view.remap_refs(map_ref);
        }
        for (_, actor) in &mut self.actors {
            actor.remap_regions(|(l, i)| node(l, i));
        }
    }

    /// Renumbers the handles and states naming the game's own entities
    /// after one was removed.
    fn remap_game_refs(&mut self, game: impl Fn(usize) -> usize) {
        let map_ref = |r: EntityRef| match r {
            EntityRef::Game(i) => EntityRef::Game(game(i)),
            other => other,
        };
        self.objects.remap_kinds(|kind| match kind {
            Kind::Entity(r) => Kind::Entity(map_ref(*r)),
            other => other.clone(),
        });
        self.game_view.remap_refs(map_ref);
    }

    /// Makes room for a node inserted at `index` in a layer.
    fn shift_nodes_up(&mut self, layer: usize, index: usize) {
        self.remap_refs(
            move |l, i| {
                if l == layer && i >= index {
                    (l, i + 1)
                } else {
                    (l, i)
                }
            },
            |l| l,
            |f| f,
        );
    }

    /// Closes the gap left by the node removed at `index` in a layer.
    fn shift_nodes_down(&mut self, layer: usize, index: usize) {
        self.remap_refs(
            move |l, i| {
                if l == layer && i > index {
                    (l, i - 1)
                } else {
                    (l, i)
                }
            },
            |l| l,
            |f| f,
        );
    }

    /// Ends what a node or free entity owns before it goes: its scripts,
    /// its sound, its sprite and region objects, its handle.
    fn drop_object(&mut self, engine: &mut Engine, id: NativeId) {
        for script in engine.scripts_of(id) {
            engine.finish(script, true);
        }
        self.stop_sfx(id, true);
        self.sounds.objects.remove(&id);
        if self.main_object == Some(id) {
            self.main_object = None;
        }
        if self.active_object == Some(id) {
            self.active_object = None;
        }
        self.forget_owned(id);
        self.objects.remove(id);
    }

    /// Sets up an entity that just joined the scene or the game: its
    /// caption is expanded, its sprites and sound loaded, its region and
    /// script properties registered, its scripts attached.
    fn init_scene_entity(&mut self, engine: &mut Engine, id: NativeId, r: EntityRef) {
        let Some(entity) = self.entity_def(r).cloned() else {
            return;
        };
        let caption = self.expand(&entity.caption);
        let mut missing = Vec::new();
        let vfs = &self.vfs;
        let view = match r {
            EntityRef::Game(_) => Some(&mut self.game_view),
            EntityRef::Ui(_) => None,
            _ => self.scene.as_mut(),
        };
        if let Some(view) = view {
            if let Some(e) = view.entity_mut(r) {
                e.caption = caption;
            }
            for sprite in entity
                .sprite
                .iter()
                .chain(&entity.talk_sprites)
                .chain(&entity.talk_special_sprites)
            {
                if !view.add_sprite(vfs, sprite) {
                    missing.push(sprite.clone());
                }
            }
        }
        for sprite in missing {
            self.log(format!("  missing: {sprite}"));
        }
        if let Some(native) = self.objects.get_mut(id) {
            for property in &entity.properties {
                native
                    .props
                    .insert(property.name.clone(), Value::from(property.value.clone()));
            }
        }
        self.load_entity_sound(id);
        self.register_entity_region(id);
        for script in entity.scripts {
            self.attach(engine, id, &script);
        }
    }

    /// Reads an `.entity` file (`CAdEntity::LoadFile`); `None`, logged,
    /// when it cannot be read or parsed.
    pub(crate) fn read_entity_def(&mut self, path: &str) -> Option<Entity> {
        match self
            .vfs
            .read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Entity::parse(&bytes).map_err(|e| e.to_string()))
        {
            Ok(def) => Some(def),
            Err(reason) => {
                self.log(format!("Error loading ENTITY file '{path}': {reason}"));
                None
            }
        }
    }

    /// Adds a free entity to the scene (`Scene.CreateEntity`,
    /// `Scene.LoadEntity`) and returns its handle; `None` without a
    /// scene.
    pub fn add_free_entity(&mut self, engine: &mut Engine, def: Entity) -> Option<NativeId> {
        let view = self.scene.as_mut()?;
        view.scene.free_entities.push(def);
        let r = EntityRef::Free(view.scene.free_entities.len() - 1);
        let id = self.objects.insert(Kind::Entity(r));
        self.init_scene_entity(engine, id, r);
        Some(id)
    }

    /// Creates an empty free entity in the scene (`Scene.CreateEntity`).
    pub fn create_scene_entity(
        &mut self,
        engine: &mut Engine,
        name: Option<&str>,
    ) -> Option<NativeId> {
        let mut def = Entity::default();
        if let Some(name) = name {
            def.name = name.to_string();
        }
        self.add_free_entity(engine, def)
    }

    /// Loads an entity file into the scene (`Scene.LoadEntity`); `None`
    /// when the file does not load.
    pub fn load_scene_entity(&mut self, engine: &mut Engine, path: &str) -> Option<NativeId> {
        let def = self.read_entity_def(path)?;
        self.add_free_entity(engine, def)
    }

    /// Removes a free entity from the scene (`UnloadObject` on one); the
    /// handles of those after it are renumbered.
    pub(crate) fn remove_free_entity(&mut self, engine: &mut Engine, id: NativeId, index: usize) {
        self.remove_attachments_of(engine, id);
        self.drop_object(engine, id);
        if let Some(view) = self.scene.as_mut() {
            if index < view.scene.free_entities.len() {
                view.scene.free_entities.remove(index);
            }
            view.forget_entity(EntityRef::Free(index));
        }
        self.remap_refs(
            |l, i| (l, i),
            |l| l,
            move |f| if f > index { f - 1 } else { f },
        );
    }

    // ----------------------------------------------------------------
    // The game's own entities and attachments
    // ----------------------------------------------------------------

    /// Adds an entity to the game rather than to a scene
    /// (`Game.CreateEntity`, `Game.LoadEntity`, `AddAttachment`): it
    /// survives scene changes. Returns its handle.
    pub fn add_game_entity(&mut self, engine: &mut Engine, def: Entity) -> NativeId {
        self.game_view.scene.free_entities.push(def);
        let r = EntityRef::Game(self.game_view.scene.free_entities.len() - 1);
        let id = self.objects.insert(Kind::Entity(r));
        self.init_scene_entity(engine, id, r);
        id
    }

    /// Creates an empty entity of the game (`Game.CreateEntity`).
    pub fn create_game_entity(&mut self, engine: &mut Engine, name: Option<&str>) -> NativeId {
        let mut def = Entity::default();
        if let Some(name) = name {
            def.name = name.to_string();
        }
        self.add_game_entity(engine, def)
    }

    /// Loads an entity file as an entity of the game (`Game.LoadEntity`);
    /// `None` when the file does not load.
    pub fn load_game_entity(&mut self, engine: &mut Engine, path: &str) -> Option<NativeId> {
        let def = self.read_entity_def(path)?;
        Some(self.add_game_entity(engine, def))
    }

    /// Removes an entity of the game (`Game.UnloadObject` on one, the
    /// end of an attachment): its own attachments go with it, its owner
    /// forgets it, and the handles of the game entities after it are
    /// renumbered.
    pub(crate) fn remove_game_entity(&mut self, engine: &mut Engine, id: NativeId, index: usize) {
        self.remove_attachments_of(engine, id);
        if let Some(owner) = self.object_state(id).and_then(|s| s.attached_to) {
            if let Some(state) = self.object_state_mut(owner) {
                state.attachments.retain(|a| a.id != id);
            }
        }
        self.drop_object(engine, id);
        // The index may have moved while the attachments were removed.
        let index = match self.objects.get(id).map(|n| &n.kind) {
            Some(Kind::Entity(EntityRef::Game(i))) => *i,
            _ => index,
        };
        if index < self.game_view.scene.free_entities.len() {
            self.game_view.scene.free_entities.remove(index);
        }
        self.game_view.forget_entity(EntityRef::Game(index));
        self.remap_game_refs(move |g| if g > index { g - 1 } else { g });
    }

    /// Attaches an entity file to an object (`AddAttachment`): the entity
    /// is the game's, drawn before (`pre`) or after the object at
    /// `(x, y)` from it, scaled and tinted with it, and hit as it.
    /// `false`, logged, when the file does not load.
    pub fn add_attachment(
        &mut self,
        engine: &mut Engine,
        owner: NativeId,
        path: &str,
        pre: bool,
        x: i32,
        y: i32,
    ) -> bool {
        let Some(mut def) = self.read_entity_def(path) else {
            self.log(format!(
                "Runtime error: AddAttachment() failed loading entity '{path}'"
            ));
            return false;
        };
        def.x = x;
        def.y = y;
        def.active = true;
        let id = self.add_game_entity(engine, def);
        if let Some(state) = self.object_state_mut(id) {
            state.attached_to = Some(owner);
        }
        if let Some(state) = self.object_state_mut(owner) {
            state
                .attachments
                .push(crate::objects::Attachment { id, pre });
        }
        true
    }

    /// The attachments of an object matching a value: by object, or every
    /// one of a name, ignoring case.
    fn matching_attachments(&self, owner: NativeId, which: &Value) -> Vec<NativeId> {
        let Some(state) = self.object_state(owner) else {
            return Vec::new();
        };
        match which {
            Value::Native(id) => state
                .attachments
                .iter()
                .map(|a| a.id)
                .filter(|a| a == id)
                .collect(),
            value => {
                let name = value.get_string(self);
                state
                    .attachments
                    .iter()
                    .map(|a| a.id)
                    .filter(|a| {
                        self.entity_def_of(*a)
                            .is_some_and(|e| e.name.eq_ignore_ascii_case(&name))
                    })
                    .collect()
            }
        }
    }

    /// The definition of an entity by handle.
    fn entity_def_of(&self, id: NativeId) -> Option<&Entity> {
        match self.objects.get(id)?.kind {
            Kind::Entity(r) => self.entity_def(r),
            _ => None,
        }
    }

    /// Removes the attachments of an object matching a value, by object
    /// or by name (`RemoveAttachment`); whether any was found.
    pub fn remove_attachment(
        &mut self,
        engine: &mut Engine,
        owner: NativeId,
        which: &Value,
    ) -> bool {
        let found = self.matching_attachments(owner, which);
        for id in &found {
            if let Some(Kind::Entity(EntityRef::Game(index))) =
                self.objects.get(*id).map(|n| n.kind.clone())
            {
                self.remove_game_entity(engine, *id, index);
            }
        }
        !found.is_empty()
    }

    /// An attachment of an object (`GetAttachment`): by index over the
    /// ones drawn before then after, or the first of a name.
    pub fn attachment(&self, owner: NativeId, which: &Value) -> Option<NativeId> {
        let state = self.object_state(owner)?;
        match which {
            Value::Int(index) => {
                let ordered = state
                    .attachments
                    .iter()
                    .filter(|a| a.pre)
                    .chain(state.attachments.iter().filter(|a| !a.pre));
                ordered.map(|a| a.id).nth(usize::try_from(*index).ok()?)
            }
            which => self.matching_attachments(owner, which).first().copied(),
        }
    }

    /// Removes every attachment of an object, once the object goes.
    pub(crate) fn remove_attachments_of(&mut self, engine: &mut Engine, owner: NativeId) {
        let attached: Vec<NativeId> = self
            .object_state(owner)
            .map(|s| s.attachments.iter().map(|a| a.id).collect())
            .unwrap_or_default();
        for id in attached {
            if let Some(Kind::Entity(EntityRef::Game(index))) =
                self.objects.get(id).map(|n| n.kind.clone())
            {
                self.remove_game_entity(engine, id, index);
            }
        }
    }

    /// The attachments of an object as drawn around it
    /// (`CAdObject::DisplaySpriteAttachment`): each at the object's
    /// position plus its own scaled offset, with the object's scale and
    /// tint. `scale` is in percent.
    pub(crate) fn attachment_draws(
        &self,
        owner: NativeId,
        x: i32,
        y: i32,
        scale: f32,
        tint: luma_formats::Rgba,
    ) -> crate::scene_view::AttachedDraws<'_> {
        let mut draws = crate::scene_view::AttachedDraws::default();
        let Some(state) = self.object_state(owner) else {
            return draws;
        };
        for attachment in &state.attachments {
            let Some(Kind::Entity(r)) = self.objects.get(attachment.id).map(|n| &n.kind) else {
                continue;
            };
            let view = &self.game_view;
            let Some(entity) = view.entity(*r) else {
                continue;
            };
            if !entity.active || entity.kind != luma_formats::EntityKind::Normal {
                continue;
            }
            let Some(loaded) = view.shown_sprite(*r).and_then(|p| view.sprite(&p)) else {
                continue;
            };
            let drawable = crate::scene_view::Drawable {
                x: x + (entity.x as f32 * scale / 100.0) as i32,
                y: y + (entity.y as f32 * scale / 100.0) as i32,
                sprite: Some(loaded),
                bank: &view.bank,
                frame: view.frame_of(*r),
                scale,
                tint,
                stick: None,
                before: Vec::new(),
                after: Vec::new(),
            };
            if attachment.pre {
                draws.before.push(drawable);
            } else {
                draws.after.push(drawable);
            }
        }
        draws
    }

    /// Adds a node to a layer (`Layer.AddEntity`, `AddRegion`,
    /// `InsertEntity`, `InsertRegion`): at `index` when given and inside
    /// the layer, else at the end. Returns the node's handle.
    pub fn add_node(
        &mut self,
        engine: &mut Engine,
        layer: usize,
        index: Option<usize>,
        node: SceneNode,
    ) -> Option<NativeId> {
        let count = self.scene()?.scene.layers.get(layer)?.nodes.len();
        let index = index.filter(|i| *i < count).unwrap_or(count);
        // Those at `index` and after move up before the new handle exists.
        self.shift_nodes_up(layer, index);
        let is_entity = matches!(node, SceneNode::Entity(_));
        self.scene_mut()?.scene.layers[layer]
            .nodes
            .insert(index, node);
        let r = EntityRef::Node { layer, index };
        let kind = if is_entity {
            Kind::Entity(r)
        } else {
            Kind::Region { layer, index }
        };
        let id = self.objects.insert(kind);
        if is_entity {
            self.init_scene_entity(engine, id, r);
        }
        Some(id)
    }

    /// Adds an entity node named `name`, scalable like the layer's others
    /// are not (`CAdEntity` in a layer).
    pub fn add_entity_node(
        &mut self,
        engine: &mut Engine,
        layer: usize,
        index: Option<usize>,
        name: Option<&str>,
    ) -> Option<NativeId> {
        let mut def = Entity {
            scalable: false,
            ..Entity::default()
        };
        if let Some(name) = name {
            def.name = name.to_string();
        }
        self.add_node(engine, layer, index, SceneNode::Entity(Box::new(def)))
    }

    /// Adds a region node named `name`.
    pub fn add_region_node(
        &mut self,
        engine: &mut Engine,
        layer: usize,
        index: Option<usize>,
        name: Option<&str>,
    ) -> Option<NativeId> {
        let mut def = Region::default();
        if let Some(name) = name {
            def.name = name.to_string();
        }
        self.add_node(engine, layer, index, SceneNode::Region(def))
    }

    /// Deletes a node of a layer (`Layer.DeleteNode`); `false` when there
    /// is none. The handles of the nodes after it are renumbered.
    pub fn delete_node(&mut self, engine: &mut Engine, layer: usize, index: usize) -> bool {
        let Some(node) = self.scene().and_then(|v| v.node(layer, index)) else {
            return false;
        };
        let id = match node {
            SceneNode::Entity(_) => self.node_id(EntityRef::Node { layer, index }),
            SceneNode::Region(_) => self.region_id(layer, index),
        };
        if let Some(id) = id {
            self.remove_attachments_of(engine, id);
            self.drop_object(engine, id);
        }
        if let Some(view) = self.scene.as_mut() {
            view.scene.layers[layer].nodes.remove(index);
            view.forget_entity(EntityRef::Node { layer, index });
        }
        self.shift_nodes_down(layer, index);
        true
    }

    /// Adds a layer (`Scene.AddLayer`, `InsertLayer`): at `index` when
    /// given and inside the list, else at the end; the size of the main
    /// layer, as the editor would give it. Returns the layer's handle.
    pub fn add_layer(&mut self, index: Option<usize>, name: Option<&str>) -> Option<NativeId> {
        let view = self.scene()?;
        let count = view.scene.layers.len();
        let index = index.filter(|i| *i < count).unwrap_or(count);
        let main = view.scene.layers.iter().rfind(|l| l.main);
        let mut layer = Layer {
            width: main.map_or(0, |m| m.width),
            height: main.map_or(0, |m| m.height),
            ..Layer::default()
        };
        if let Some(name) = name {
            layer.name = name.to_string();
        }
        self.remap_refs(
            move |l, i| if l >= index { (l + 1, i) } else { (l, i) },
            move |l| if l >= index { l + 1 } else { l },
            |f| f,
        );
        self.scene_mut()?.scene.layers.insert(index, layer);
        Some(self.objects.insert(Kind::Layer(index)))
    }

    /// Deletes a layer with its nodes (`Scene.DeleteLayer`); the main
    /// layer cannot go. `false` when nothing was deleted.
    pub fn delete_layer(&mut self, engine: &mut Engine, index: usize) -> bool {
        let Some(layer) = self.scene().and_then(|v| v.scene.layers.get(index)) else {
            return false;
        };
        if layer.main {
            self.log("Runtime error: Scene.DeleteLayer - cannot delete main scene layer");
            return false;
        }
        let nodes = layer.nodes.len();
        for ni in (0..nodes).rev() {
            self.delete_node(engine, index, ni);
        }
        if let Some(id) = self.layer_id(index) {
            self.drop_object(engine, id);
        }
        if let Some(view) = self.scene.as_mut() {
            view.scene.layers.remove(index);
        }
        self.remap_refs(
            move |l, i| if l > index { (l - 1, i) } else { (l, i) },
            move |l| if l > index { l - 1 } else { l },
            |f| f,
        );
        true
    }

    /// Makes a free object (an actor or a free entity) draw in the slot
    /// of a region of the main layer rather than by its position
    /// (`StickToRegion`): the region by handle or by name, `None` to
    /// unstick. Returns whether the region was found (or none asked).
    pub fn stick_to_region(&mut self, id: NativeId, region: &Value) -> bool {
        if !matches!(
            self.objects.get(id).map(|n| &n.kind),
            Some(Kind::Actor | Kind::Entity(_))
        ) {
            return false;
        }
        let main = self
            .scene()
            .and_then(|v| v.scene.layers.iter().rposition(|l| l.main));
        let found = match (region, main) {
            (Value::Null, _) | (_, None) => Some(None),
            (Value::Native(handle), Some(main)) => match self.objects.get(*handle).map(|n| &n.kind)
            {
                Some(Kind::Region { layer, .. }) if *layer == main => Some(Some(*handle)),
                _ => None,
            },
            (value, Some(main)) => {
                let name = value.get_string(self);
                let view = self.scene().expect("a main layer needs a scene");
                view.scene.layers[main]
                    .nodes
                    .iter()
                    .position(
                        |n| matches!(n, SceneNode::Region(r) if r.name.eq_ignore_ascii_case(&name)),
                    )
                    .and_then(|ni| self.region_id(main, ni))
                    .map(Some)
            }
        };
        if let Some(state) = self.object_state_mut(id) {
            state.stick_region = found.flatten();
        }
        found.is_some()
    }

    /// The region of the main layer a free object sticks to, as a
    /// `(layer, node)` pair.
    pub(crate) fn stick_region_of(&self, id: NativeId) -> Option<(usize, usize)> {
        let handle = self.object_state(id)?.stick_region?;
        match self.objects.get(handle)?.kind {
            Kind::Region { layer, index } => Some((layer, index)),
            _ => None,
        }
    }
}
