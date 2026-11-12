//! A small TrueType font built in code, so that the tests of the text
//! layout do not depend on the fonts of the machine: every character it
//! holds is a box 300 units wide and 700 high, advancing by half an em of
//! 1000 units; the space advances as much and has no ink.

/// What a test font is made of.
pub(crate) struct TestFont<'a> {
    /// The family name (`FACE`).
    pub family: &'a str,
    /// The characters the font has a glyph for.
    pub chars: &'a str,
    /// The weight: 400 regular, 700 bold.
    pub weight: u16,
    /// Whether the face says it is italic.
    pub italic: bool,
}

impl TestFont<'_> {
    /// A regular face of a family with these characters.
    pub(crate) fn regular<'a>(family: &'a str, chars: &'a str) -> TestFont<'a> {
        TestFont {
            family,
            chars,
            weight: 400,
            italic: false,
        }
    }

    /// The bytes of the `.ttf` file.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut chars: Vec<char> = self.chars.chars().collect();
        chars.sort_unstable();
        chars.dedup();
        let glyphs = chars.len() as u16 + 1;

        let mut head = Vec::new();
        put32(&mut head, 0x0001_0000); // version
        put32(&mut head, 0x0001_0000); // revision
        put32(&mut head, 0); // checksum adjustment
        put32(&mut head, 0x5F0F_3CF5); // magic
        put16(&mut head, 0); // flags
        put16(&mut head, 1000); // units per em
        head.extend_from_slice(&[0; 16]); // created, modified
        for v in [0i16, -200, 500, 800] {
            put16(&mut head, v as u16); // bounding box
        }
        put16(&mut head, 0); // mac style
        put16(&mut head, 8); // lowest size
        put16(&mut head, 2); // direction hint
        put16(&mut head, 0); // short offsets in `loca`
        put16(&mut head, 0); // glyph data format

        let mut hhea = Vec::new();
        put32(&mut hhea, 0x0001_0000);
        put16(&mut hhea, 800); // ascender
        put16(&mut hhea, (-200i16) as u16); // descender
        put16(&mut hhea, 0); // line gap
        put16(&mut hhea, 500); // widest advance
        hhea.extend_from_slice(&[0; 22]);
        put16(&mut hhea, glyphs); // number of metrics

        let mut maxp = Vec::new();
        put32(&mut maxp, 0x0001_0000);
        put16(&mut maxp, glyphs);
        maxp.extend_from_slice(&[0; 26]);

        let mut hmtx = Vec::new();
        for _ in 0..glyphs {
            put16(&mut hmtx, 500);
            put16(&mut hmtx, 0);
        }

        let mut cmap = Vec::new();
        put16(&mut cmap, 0); // version
        put16(&mut cmap, 1); // one subtable
        put16(&mut cmap, 3); // Windows
        put16(&mut cmap, 10); // full Unicode
        put32(&mut cmap, 12); // its offset
        put16(&mut cmap, 12); // format 12
        put16(&mut cmap, 0);
        put32(&mut cmap, 16 + 12 * chars.len() as u32);
        put32(&mut cmap, 0); // language
        put32(&mut cmap, chars.len() as u32);
        for (i, c) in chars.iter().enumerate() {
            put32(&mut cmap, u32::from(*c));
            put32(&mut cmap, u32::from(*c));
            put32(&mut cmap, i as u32 + 1);
        }

        // One contour: the box from (100, 0) to (400, 700).
        let mut outline = Vec::new();
        put16(&mut outline, 1);
        for v in [100u16, 0, 400, 700] {
            put16(&mut outline, v);
        }
        put16(&mut outline, 3); // last point of the contour
        put16(&mut outline, 0); // no instructions
        outline.extend_from_slice(&[1; 4]); // four points on the curve
        for v in [100i16, 0, 300, 0, 0, 700, 0, -700] {
            put16(&mut outline, v as u16);
        }
        let (mut glyf, mut loca) = (Vec::new(), Vec::new());
        put16(&mut loca, 0); // the missing glyph: empty
        put16(&mut loca, 0);
        for c in &chars {
            if *c != ' ' {
                glyf.extend_from_slice(&outline);
            }
            put16(&mut loca, (glyf.len() / 2) as u16);
        }

        let style = match (self.weight >= 600, self.italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            (false, false) => "Regular",
        };
        let names = [
            (1u16, self.family.to_string()),
            (2, style.to_string()),
            (4, format!("{} {style}", self.family)),
            (6, format!("{}-{}", self.family, style).replace(' ', "")),
        ];
        let (mut name, mut strings) = (Vec::new(), Vec::new());
        put16(&mut name, 0);
        put16(&mut name, names.len() as u16);
        put16(&mut name, 6 + 12 * names.len() as u16);
        for (id, text) in &names {
            let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            for v in [3, 1, 0x0409, *id, utf16.len() as u16, strings.len() as u16] {
                put16(&mut name, v);
            }
            strings.extend_from_slice(&utf16);
        }
        name.extend_from_slice(&strings);

        let mut os2 = Vec::new();
        put16(&mut os2, 0); // version
        put16(&mut os2, 500); // average width
        put16(&mut os2, self.weight);
        put16(&mut os2, 5); // normal width
        os2.extend_from_slice(&[0; 18]); // embedding, sub- and superscripts
        put16(&mut os2, 50); // strike-out size
        put16(&mut os2, 300); // strike-out position
        os2.extend_from_slice(&[0; 32]); // class, panose, ranges, vendor
        let selection = u16::from(self.italic) | if self.weight >= 600 { 0x20 } else { 0 };
        put16(&mut os2, if selection == 0 { 0x40 } else { selection });
        put16(&mut os2, 0x20);
        put16(&mut os2, 0xFFFF);
        for v in [800i16, -200, 0, 800, 200] {
            put16(&mut os2, v as u16);
        }

        let mut post = Vec::new();
        put32(&mut post, 0x0003_0000);
        put32(&mut post, 0); // italic angle
        put16(&mut post, (-100i16) as u16); // underline position
        put16(&mut post, 50); // underline thickness
        post.extend_from_slice(&[0; 20]);

        let mut tables = [
            (*b"OS/2", os2),
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
            (*b"name", name),
            (*b"post", post),
        ];
        tables.sort_by_key(|(tag, _)| *tag);
        let mut file = Vec::new();
        put32(&mut file, 0x0001_0000);
        put16(&mut file, tables.len() as u16);
        file.extend_from_slice(&[0; 6]); // search hints, unused
        let mut offset = 12 + 16 * tables.len();
        for (tag, data) in &tables {
            file.extend_from_slice(tag);
            put32(&mut file, 0); // checksum, not verified
            put32(&mut file, offset as u32);
            put32(&mut file, data.len() as u32);
            offset += data.len().next_multiple_of(4);
        }
        for (_, data) in &tables {
            file.extend_from_slice(data);
            file.resize(file.len().next_multiple_of(4), 0);
        }
        file
    }
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}
