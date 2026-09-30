use lodeframe::protocol::Encode;

#[derive(Encode)]
#[lodeframe(nope)]
struct S;

fn main() {}
