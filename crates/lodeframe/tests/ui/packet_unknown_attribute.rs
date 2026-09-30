use lodeframe::protocol::Packet;

#[derive(Packet)]
#[packet(id = 1, state = Play, side = Clientbound, name = "x")]
struct P;

fn main() {}
