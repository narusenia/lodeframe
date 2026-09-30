use lodeframe::protocol::Decode;

#[derive(Decode)]
enum E {
    A = 1 + 1,
}

fn main() {}
