use lodeframe::protocol::Encode;

#[derive(Encode)]
union U {
    a: u8,
    b: u16,
}

fn main() {}
