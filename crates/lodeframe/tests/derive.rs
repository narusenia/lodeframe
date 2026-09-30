// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The derives must produce the same bytes as hand-written impls, through the facade path.

use lodeframe::protocol::{Decode, Encode, Error, Packet, Side, State, VarInt};

fn encoded(v: &impl Encode) -> Vec<u8> {
    let mut buf = Vec::new();
    v.encode(&mut buf).unwrap();
    buf
}

fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(v: T) {
    let buf = encoded(&v);
    let mut r = buf.as_slice();
    assert_eq!(T::decode(&mut r).unwrap(), v);
    assert!(r.is_empty());
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Named {
    id: VarInt,
    name: String,
    hp: Option<u16>,
    tags: Vec<u8>,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Tuple(u8, bool);

#[derive(Debug, PartialEq, Encode, Decode)]
struct Unit;

#[test]
fn struct_fields_are_written_in_declaration_order() {
    let v = Named {
        id: VarInt(300),
        name: "ab".into(),
        hp: Some(7),
        tags: vec![9, 8],
    };
    // hand-written equivalent
    let mut expected = Vec::new();
    VarInt(300).encode(&mut expected).unwrap();
    "ab".encode(&mut expected).unwrap();
    Some(7u16).encode(&mut expected).unwrap();
    vec![9u8, 8].encode(&mut expected).unwrap();
    assert_eq!(encoded(&v), expected);
    assert_eq!(encoded(&v), [0xac, 0x02, 2, b'a', b'b', 1, 0, 7, 2, 9, 8]);
    roundtrip(v);
}

#[test]
fn tuple_and_unit_structs() {
    assert_eq!(encoded(&Tuple(5, true)), [5, 1]);
    roundtrip(Tuple(5, true));
    assert_eq!(encoded(&Unit), Vec::<u8>::new());
    roundtrip(Unit);
}

#[derive(Debug, PartialEq, Encode, Decode)]
enum Shape {
    Empty,
    Point(i32, i32),
    Named { w: u8, r: u8 },
}

#[test]
fn enum_is_a_varint_tag_then_fields() {
    assert_eq!(encoded(&Shape::Empty), [0]);
    assert_eq!(encoded(&Shape::Point(1, 2)), [1, 0, 0, 0, 1, 0, 0, 0, 2]);
    assert_eq!(encoded(&Shape::Named { w: 3, r: 4 }), [2, 3, 4]);
    roundtrip(Shape::Empty);
    roundtrip(Shape::Point(-1, 1));
    // fields named like the writer / reader must not shadow them
    roundtrip(Shape::Named { w: 3, r: 4 });
}

#[derive(Debug, PartialEq, Encode, Decode)]
enum Explicit {
    A = 3,
    B,
    C = -1,
    D = 0x100,
}

#[test]
fn explicit_discriminants_follow_rust_rules() {
    assert_eq!(encoded(&Explicit::A), [3]);
    assert_eq!(encoded(&Explicit::B), [4]);
    assert_eq!(encoded(&Explicit::C), [0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert_eq!(encoded(&Explicit::D), [0x80, 0x02]);
    for v in [Explicit::A, Explicit::B, Explicit::C, Explicit::D] {
        roundtrip(v);
    }
}

#[test]
fn unknown_tag_and_truncation_are_errors() {
    let err = Shape::decode(&mut [9u8].as_slice()).unwrap_err();
    assert!(
        matches!(err, Error::InvalidValue(m) if m.contains("Shape")),
        "{err}"
    );
    assert!(matches!(
        Shape::decode(&mut [1u8, 0, 0].as_slice()),
        Err(Error::UnexpectedEof)
    ));
    assert!(matches!(
        Named::decode(&mut [].as_slice()),
        Err(Error::UnexpectedEof)
    ));
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Wrapper<T> {
    inner: T,
    rest: Vec<T>,
}

#[test]
fn generics_get_bounds() {
    roundtrip(Wrapper {
        inner: 1u8,
        rest: vec![2, 3],
    });
    roundtrip(Wrapper {
        inner: "x".to_string(),
        rest: vec![],
    });
}

#[derive(Packet, Encode, Decode)]
#[packet(id = 0x26, state = Play, side = Clientbound)]
struct KeepAlive(i64);

#[derive(Packet)]
#[packet(id = 0, state = Handshake, side = Serverbound)]
struct Handshake;

#[test]
fn packet_constants() {
    assert_eq!(KeepAlive::ID, 0x26);
    assert_eq!(KeepAlive::STATE, State::Play);
    assert_eq!(KeepAlive::SIDE, Side::Clientbound);
    assert_eq!(
        (Handshake::ID, Handshake::STATE, Handshake::SIDE),
        (0, State::Handshake, Side::Serverbound)
    );
}
