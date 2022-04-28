pub mod onertt;

pub enum OvertProtocol {
    // ZeroRtt(zerortt::ProtocolSpec),
    OneRtt(onertt::ProtocolSpec),
    // TwoRtt(twortt::ProtocolSpec),
}
