use lodeframe::protocol::Packet;

#[derive(Packet)]
#[packet(id = 1, state = Play)]
struct P;

fn main() {}
