/// File-reference encoding used by FFNA/model/audio chunks in legacy GW DAT tools.
///
/// Confirmed in `references/GuildWarsMapBrowser-master/SourceFiles/Parsers/FileReferenceParser.h`
/// and `SoundEventParser.h`: `file_id = (id0 - 0xFF00FF) + (id1 * 0xFF00)`.
/// The arithmetic is intentionally wrapping because the legacy implementation casts the
/// signed result back to an unsigned DAT file id.
pub(crate) fn decode_file_reference(id0: u16, id1: u16) -> u32 {
    u32::from(id0)
        .wrapping_sub(0x00ff_00ff)
        .wrapping_add(u32::from(id1).wrapping_mul(0x0000_ff00))
}

#[cfg(test)]
pub(crate) fn encode_file_reference(file_id: u32) -> (u16, u16) {
    let zero_based = file_id.wrapping_sub(1);
    let id0 = ((zero_based % 0xff00) + 0x100) as u16;
    let id1 = ((zero_based / 0xff00) + 0x100) as u16;
    (id0, id1)
}
