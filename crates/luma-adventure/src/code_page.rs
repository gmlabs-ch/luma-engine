//! The Windows code pages an ANSI game's text is read in: the one a
//! TrueType font's `CHARSET` names, and the one the system the game was
//! made for used everywhere else (see `docs/formats/font.md`).
//!
//! Written with reference to `BFontTT.cpp` (`GetWindowsCharset`) and
//! `BTextUtils.cpp` of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`. The original engine leaves the
//! conversions to Windows: GDI reads the bytes given to a font in the
//! code page of the font's character set, and `MultiByteToWideChar` and
//! `WideCharToMultiByte` use the system's ANSI code page (`CP_ACP`).

use encoding_rs::Encoding;

/// A Windows code page: how the bytes of ANSI text stand for characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodePage(u16);

/// The number Windows gives the code page of symbol fonts (`CP_SYMBOL`).
const SYMBOL: u16 = 42;

impl CodePage {
    /// Windows-1252, Western European: the code page of `ANSI_CHARSET`
    /// and of the systems most games were made on.
    pub const WESTERN: CodePage = CodePage(1252);

    /// The code page of a Windows number, when Luma Engine can read it:
    /// 874 (Thai), 932 (Japanese), 936 (Simplified Chinese), 949
    /// (Korean), 950 (Traditional Chinese), 1250 to 1258 (Central
    /// European, Cyrillic, Western, Greek, Turkish, Hebrew, Arabic,
    /// Baltic, Vietnamese), 10000 (Mac Roman) and 42 (symbol fonts).
    pub fn new(number: u16) -> Option<CodePage> {
        let page = CodePage(number);
        (number == SYMBOL || page.encoding().is_some()).then_some(page)
    }

    /// The Windows number of the code page.
    pub fn number(self) -> u16 {
        self.0
    }

    /// The code page GDI reads a font's text in, from the font's
    /// `CHARSET` (`CBFontTT::GetWindowsCharset` gives the Windows
    /// character set; each has its code page). `system` is the ANSI code
    /// page of the system, which `DEFAULT_CHARSET` (1) and `OEM_CHARSET`
    /// (2) stand for. A number outside 1 to 18 is `ANSI_CHARSET`:
    /// Windows-1252.
    ///
    /// Two character sets have no code page here and are read as the
    /// system's: `OEM_CHARSET`, whose page is the system's DOS code page
    /// (437, 850...), and `JOHAB_CHARSET` (15, code page 1361).
    pub fn of_charset(charset: i32, system: CodePage) -> CodePage {
        match CodePage::named_by_charset(charset) {
            Some(page) => page,
            None if matches!(charset, 1 | 2 | 15) => system,
            None => CodePage::WESTERN,
        }
    }

    /// The code page a `CHARSET` names by itself: `None` for the ones
    /// that stand for the system's (1, 2), for `ANSI_CHARSET` (any other
    /// number) and for Johab (15).
    pub fn named_by_charset(charset: i32) -> Option<CodePage> {
        let number = match charset {
            3 => 1257,    // BALTIC_CHARSET
            4 => 950,     // CHINESEBIG5_CHARSET
            5 => 1250,    // EASTEUROPE_CHARSET
            6 => 936,     // GB2312_CHARSET
            7 => 1253,    // GREEK_CHARSET
            8 => 949,     // HANGUL_CHARSET
            9 => 10000,   // MAC_CHARSET
            10 => 1251,   // RUSSIAN_CHARSET
            11 => 932,    // SHIFTJIS_CHARSET
            12 => SYMBOL, // SYMBOL_CHARSET
            13 => 1254,   // TURKISH_CHARSET
            14 => 1258,   // VIETNAMESE_CHARSET
            16 => 1256,   // ARABIC_CHARSET
            17 => 1255,   // HEBREW_CHARSET
            18 => 874,    // THAI_CHARSET
            _ => return None,
        };
        Some(CodePage(number))
    }

    /// Whether a `CHARSET` says which language the game is written in,
    /// and so which code page its system had: every one that names a
    /// code page except the Macintosh's and the symbol fonts'.
    pub fn system_of_charset(charset: i32) -> Option<CodePage> {
        CodePage::named_by_charset(charset).filter(|page| !matches!(page.0, 10000 | SYMBOL))
    }

    fn encoding(self) -> Option<&'static Encoding> {
        Some(match self.0 {
            874 => encoding_rs::WINDOWS_874,
            932 => encoding_rs::SHIFT_JIS,
            936 => encoding_rs::GBK,
            949 => encoding_rs::EUC_KR,
            950 => encoding_rs::BIG5,
            1250 => encoding_rs::WINDOWS_1250,
            1251 => encoding_rs::WINDOWS_1251,
            1252 => encoding_rs::WINDOWS_1252,
            1253 => encoding_rs::WINDOWS_1253,
            1254 => encoding_rs::WINDOWS_1254,
            1255 => encoding_rs::WINDOWS_1255,
            1256 => encoding_rs::WINDOWS_1256,
            1257 => encoding_rs::WINDOWS_1257,
            1258 => encoding_rs::WINDOWS_1258,
            10000 => encoding_rs::MACINTOSH,
            _ => return None,
        })
    }

    /// The characters of ANSI text (`CBTextUtils::AnsiToUnicode`, and
    /// what GDI does with the bytes given to a font): `raw` holds one
    /// `char` per byte, as the engine holds the game's text. A byte, or
    /// a pair of bytes, the code page does not define becomes U+FFFD,
    /// except the control codes Windows passes through. A character
    /// beyond U+00FF is not a byte and is kept as it is.
    pub fn decode(self, raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        let mut bytes: Vec<u8> = Vec::new();
        for c in raw.chars() {
            match u8::try_from(u32::from(c)) {
                Ok(byte) => bytes.push(byte),
                Err(_) => {
                    self.decode_bytes(&bytes, &mut out);
                    bytes.clear();
                    out.push(c);
                }
            }
        }
        self.decode_bytes(&bytes, &mut out);
        out
    }

    fn decode_bytes(self, bytes: &[u8], out: &mut String) {
        if bytes.is_empty() {
            return;
        }
        match self.encoding() {
            Some(encoding) => out.push_str(&encoding.decode_without_bom_handling(bytes).0),
            // Symbol fonts have their glyphs at U+F020 to U+F0FF.
            None => out.extend(bytes.iter().map(|&b| match b {
                0x20..=0xFF => char::from_u32(0xF000 + u32::from(b)).unwrap_or(char::from(b)),
                _ => char::from(b),
            })),
        }
    }

    /// The bytes of a text in the code page (`CBTextUtils::Utf8ToAnsi`,
    /// once the text is characters): `?` for a character the code page
    /// lacks. Windows also replaces some of those by a look-alike (`Ā`
    /// by `A`); Luma Engine does not.
    pub fn encode(self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len());
        let mut utf8 = [0u8; 4];
        let mut buffer = [0u8; 16];
        for c in text.chars() {
            if c.is_ascii() {
                out.push(c as u8);
                continue;
            }
            let Some(encoding) = self.encoding() else {
                // A symbol font's character goes back to its byte.
                out.push(match u32::from(c) {
                    code @ 0xF020..=0xF0FF => (code - 0xF000) as u8,
                    _ => b'?',
                });
                continue;
            };
            let mut encoder = encoding.new_encoder();
            let (result, _, written) = encoder.encode_from_utf8_without_replacement(
                c.encode_utf8(&mut utf8),
                &mut buffer,
                true,
            );
            match result {
                encoding_rs::EncoderResult::InputEmpty => out.extend_from_slice(&buffer[..written]),
                _ => out.push(b'?'),
            }
        }
        out
    }

    /// The text as an ANSI game holds it: one `char` per byte of the
    /// code page. A character the code page lacks is `?`, as for
    /// [`encode`](Self::encode), unless it is beyond U+00FF: it then
    /// cannot be taken for a byte and stays as it is (the fonts draw
    /// such a character as it is too).
    pub fn encode_text(self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut utf8 = [0u8; 4];
        for c in text.chars() {
            match self.encode(c.encode_utf8(&mut utf8)).as_slice() {
                [b'?'] if c != '?' && u32::from(c) > 0xFF => out.push(c),
                bytes => out.extend(bytes.iter().map(|&b| char::from(b))),
            }
        }
        out
    }

    /// The byte of a character, when the code page has it as one byte.
    pub fn byte(self, c: char) -> Option<u8> {
        let mut utf8 = [0u8; 4];
        match self.encode(c.encode_utf8(&mut utf8)).as_slice() {
            [b'?'] if c != '?' => None,
            [byte] => Some(*byte),
            _ => None,
        }
    }
}

impl Default for CodePage {
    fn default() -> Self {
        CodePage::WESTERN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game's ANSI text: one `char` per byte.
    fn raw(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| char::from(b)).collect()
    }

    #[test]
    fn a_charset_names_its_code_page() {
        let system = CodePage::new(1251).unwrap();
        let page = |charset| CodePage::of_charset(charset, system).number();
        // `GetWindowsCharset`: 1 to 18, anything else is ANSI.
        assert_eq!(page(0), 1252);
        assert_eq!(page(19), 1252);
        assert_eq!(page(-3), 1252);
        assert_eq!(page(1), 1251, "the system's");
        assert_eq!(page(2), 1251, "the system's, for want of the DOS page");
        let named = [
            (3, 1257),
            (4, 950),
            (5, 1250),
            (6, 936),
            (7, 1253),
            (8, 949),
            (9, 10000),
            (10, 1251),
            (11, 932),
            (12, 42),
            (13, 1254),
            (14, 1258),
            (16, 1256),
            (17, 1255),
            (18, 874),
        ];
        for (charset, number) in named {
            assert_eq!(page(charset), number, "charset {charset}");
            assert!(CodePage::new(number).is_some());
        }
        assert_eq!(page(15), 1251, "Johab is not read");
        assert_eq!(CodePage::new(437), None);
        // What a font tells of the game's system.
        assert_eq!(
            CodePage::system_of_charset(5).map(CodePage::number),
            Some(1250)
        );
        for charset in [0, 1, 2, 9, 12, 15, 40] {
            assert_eq!(CodePage::system_of_charset(charset), None);
        }
    }

    #[test]
    fn ansi_bytes_are_read_in_the_code_page() {
        let page = |n| CodePage::new(n).unwrap();
        // The same bytes in four code pages.
        let bytes = raw(b"\xe6\xe8\xf9 \x9a");
        assert_eq!(page(1252).decode(&bytes), "æèù š");
        assert_eq!(page(1250).decode(&bytes), "ćčů š");
        assert_eq!(page(1251).decode(&bytes), "жищ љ");
        assert_eq!(page(1253).decode(&bytes), "ζθω \u{9a}");
        assert_eq!(page(1254).decode(&raw(b"\xf0\xfd\xfe")), "ğış");
        assert_eq!(page(1257).decode(&raw(b"\xe0\xe8")), "ąč");
        assert_eq!(page(1255).decode(&raw(b"\xf9\xec\xe5\xed")), "שלום");
        assert_eq!(page(1256).decode(&raw(b"\xd3\xe1\xc7\xe3")), "سلام");
        assert_eq!(page(874).decode(&raw(b"\xa1\xd2")), "กา");
        // Two bytes a character.
        assert_eq!(page(932).decode(&raw(b"\x93\xfa\x96\x7b A")), "日本 A");
        assert_eq!(page(936).decode(&raw(b"\xd6\xd0\xce\xc4")), "中文");
        assert_eq!(page(949).decode(&raw(b"\xc7\xd1\xb1\xdb")), "한글");
        assert_eq!(page(950).decode(&raw(b"\xa4\xa4\xa4\xe5")), "中文");
        assert_eq!(page(10000).decode(&raw(b"\x8e")), "é");
        // Symbol fonts: the private characters the glyphs sit at.
        assert_eq!(page(42).decode(&raw(b"A\xa7")), "\u{f041}\u{f0a7}");
        // A byte the page does not define, a pair cut short: no panic.
        assert_eq!(page(932).decode(&raw(b"\x93")), "\u{fffd}");
        assert_eq!(page(1252).decode(&raw(b"\x81")), "\u{81}");
        // A character that is no byte is kept.
        assert_eq!(page(1251).decode("\u{e6}ж\u{e6}"), "жжж");
        assert_eq!(page(1251).decode(""), "");
    }

    #[test]
    fn characters_go_back_to_the_bytes_of_the_code_page() {
        let page = |n| CodePage::new(n).unwrap();
        assert_eq!(page(1252).encode("café „x“ €"), b"caf\xe9 \x84x\x93 \x80");
        assert_eq!(page(1251).encode("жищ é"), b"\xe6\xe8\xf9 ?");
        assert_eq!(page(1250).encode("ćčů"), b"\xe6\xe8\xf9");
        assert_eq!(page(1253).encode("ζθω ж"), b"\xe6\xe8\xf9 ?");
        assert_eq!(page(932).encode("日本 A"), b"\x93\xfa\x96\x7b A");
        assert_eq!(page(42).encode("\u{f041}é"), b"A?");
        assert_eq!(page(1252).encode("\u{fffd}"), b"?");
        assert_eq!(page(1251).encode_text("жи"), "\u{e6}\u{e8}");
        // What the page lacks: kept beyond a byte, `?` within.
        assert_eq!(page(1252).encode_text("aжé?"), "aж\u{e9}?");
        assert_eq!(page(1251).encode_text("é"), "?");
        assert_eq!(page(932).encode_text("日"), "\u{93}\u{fa}");
        assert_eq!(page(1251).decode(&page(1251).encode_text("Жук ω")), "Жук ω");
        // One byte for a key.
        assert_eq!(page(1251).byte('ж'), Some(0xe6));
        assert_eq!(page(1252).byte('ж'), None);
        assert_eq!(page(1252).byte('?'), Some(b'?'));
        assert_eq!(page(932).byte('日'), None, "two bytes");
        assert_eq!(page(1252).byte('€'), Some(0x80));
    }
}
