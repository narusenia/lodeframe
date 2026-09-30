// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::io::Write;

use lodeframe_text::Component;

use crate::{Compound, Encode, Nbt, Result};

/// Plain text is a bare string; anything styled is a compound, as the game expects.
impl From<&Component> for Nbt {
    fn from(c: &Component) -> Self {
        if c.style.is_empty() {
            return Self::String(c.text.clone());
        }
        let mut out = Compound::new();
        out.insert("text", Self::String(c.text.clone()));
        if let Some(color) = c.style.color {
            out.insert("color", Self::String(color.to_string()));
        }
        for (key, value) in [
            ("bold", c.style.bold),
            ("italic", c.style.italic),
            ("underlined", c.style.underlined),
            ("strikethrough", c.style.strikethrough),
            ("obfuscated", c.style.obfuscated),
        ] {
            if let Some(v) = value {
                out.insert(key, Self::Byte(i8::from(v)));
            }
        }
        Self::Compound(out)
    }
}

impl Encode for Component {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        Nbt::from(self).encode(w)
    }
}

#[cfg(test)]
mod tests {
    use lodeframe_text::Color;

    use super::*;
    use crate::codec::tests::encoded;

    #[test]
    fn plain_text_is_a_string_tag() {
        assert_eq!(encoded(&Component::text("hi")), [8, 0, 2, b'h', b'i']);
    }

    #[test]
    fn styled_text_is_a_compound() {
        let c = Component::text("hi").color(Color::Red).bold();
        let Nbt::Compound(out) = Nbt::from(&c) else {
            panic!("expected a compound")
        };
        assert_eq!(out.get("text"), Some(&Nbt::from("hi")));
        assert_eq!(out.get("color"), Some(&Nbt::from("red")));
        assert_eq!(out.get("bold"), Some(&Nbt::Byte(1)));
        assert_eq!(out.get("italic"), None);
        assert_eq!(out.0.len(), 3);
    }

    #[test]
    fn rgb_colour_is_hex() {
        let c = Component::text("x").color(Color::Rgb(0xff, 0x00, 0xa0));
        let Nbt::Compound(out) = Nbt::from(&c) else {
            panic!("expected a compound")
        };
        assert_eq!(out.get("color"), Some(&Nbt::from("#ff00a0")));
    }

    #[test]
    fn styled_text_survives_a_nbt_roundtrip() {
        let c = Component::text("a\0🦀")
            .color(Color::Gold)
            .italic()
            .obfuscated();
        let bytes = encoded(&c);
        let decoded = Nbt::decode(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded, Nbt::from(&c));
        let Nbt::Compound(out) = decoded else {
            panic!("expected a compound")
        };
        assert_eq!(out.get("obfuscated"), Some(&Nbt::Byte(1)));
    }

    use crate::Decode;
}
