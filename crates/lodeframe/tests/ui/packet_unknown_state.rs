use lodeframe::protocol::Packet;

#[derive(Packet)]
#[packet(id = 1, state = Lobby, side = Clientbound)]
struct P;

fn main() {}
