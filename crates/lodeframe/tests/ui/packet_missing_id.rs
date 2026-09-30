use lodeframe::protocol::Packet;

#[derive(Packet)]
#[packet(state = Play, side = Clientbound)]
struct P;

fn main() {}
