use lodeframe::protocol::Packet;

#[derive(Packet)]
#[packet(id = 1, state = Play, side = Both)]
struct P;

fn main() {}
