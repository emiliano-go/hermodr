//! Rewraps the Opus packets of a WebM recording into Ogg, the container
//! WhatsApp's voice notes use. WebView2 (Chromium) can only record WebM, and
//! the audio itself is already Opus, so no re-encoding is needed.

const EBML_SEGMENT: u32 = 0x1853_8067;
const EBML_CLUSTER: u32 = 0x1F43_B675;
const EBML_TRACKS: u32 = 0x1654_AE6B;
const EBML_TRACK_ENTRY: u32 = 0xAE;
const EBML_BLOCK_GROUP: u32 = 0xA0;
const EBML_CODEC_PRIVATE: u32 = 0x63A2;
const EBML_SIMPLE_BLOCK: u32 = 0xA3;
const EBML_BLOCK: u32 = 0xA1;

/// Reads an EBML variable-length integer; `keep_marker` keeps the length bit (element IDs).
fn vint(data: &[u8], at: usize, keep_marker: bool) -> Option<(u64, usize, bool)> {
    let first = *data.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || at + len > data.len() {
        return None;
    }
    // `len` is at most 8, where the mask is zero; a u8 shift by 8 would overflow.
    let mask = (0xFFu16 >> len) as u8;
    let mut value = if keep_marker { first as u64 } else { (first & mask) as u64 };
    let mut all_ones = value == mask as u64;
    for &b in &data[at + 1..at + len] {
        value = (value << 8) | b as u64;
        all_ones &= b == 0xFF;
    }
    Some((value, len, all_ones && !keep_marker))
}

/// The codec header and the Opus packets, in order.
fn demux_webm(data: &[u8]) -> Option<(Option<Vec<u8>>, Vec<Vec<u8>>)> {
    let mut head = None;
    let mut packets = Vec::new();
    let mut at = 0;
    // Containers are entered rather than skipped, which also copes with the
    // unknown sizes a live recording writes for the segment and clusters.
    while at < data.len() {
        let (id, id_len, _) = vint(data, at, true)?;
        let (size, size_len, unknown) = vint(data, at + id_len, false)?;
        let body = at + id_len + size_len;
        match id as u32 {
            EBML_SEGMENT | EBML_CLUSTER | EBML_TRACKS | EBML_TRACK_ENTRY | EBML_BLOCK_GROUP => {
                at = body;
                continue;
            }
            _ if unknown => return None,
            _ => {}
        }
        let end = body.checked_add(size as usize)?.min(data.len());
        match id as u32 {
            EBML_CODEC_PRIVATE => head = Some(data[body..end].to_vec()),
            EBML_SIMPLE_BLOCK | EBML_BLOCK => {
                // Track number, 16-bit timecode, flags, then the frame.
                let (_, track_len, _) = vint(data, body, false)?;
                let flags = *data.get(body + track_len + 2)?;
                let frame = body + track_len + 3;
                // Chromium never laces audio; a laced block is not worth guessing at.
                if flags & 0x06 == 0 && frame < end {
                    packets.push(data[frame..end].to_vec());
                }
            }
            _ => {}
        }
        at = end;
    }
    Some((head, packets))
}

/// Samples at 48 kHz that one Opus packet decodes to (RFC 6716 §3.1).
fn packet_samples(packet: &[u8]) -> u64 {
    let Some(&toc) = packet.first() else { return 0 };
    let config = toc >> 3;
    // Frame length in units of 2.5 ms.
    let frame = match config {
        0..=11 => [4, 8, 16, 24][(config % 4) as usize],
        12..=15 => [4, 8][(config % 2) as usize],
        _ => [1, 2, 4, 8][(config % 4) as usize],
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => packet.get(1).map_or(0, |c| (c & 0x3F) as u64),
    };
    frames * frame * 120
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &byte in data {
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04C1_1DB7 } else { crc << 1 };
        }
    }
    crc
}

struct OggWriter {
    out: Vec<u8>,
    sequence: u32,
}

impl OggWriter {
    fn page(&mut self, packets: &[&[u8]], granule: u64, flags: u8) {
        let mut lacing = Vec::new();
        for packet in packets {
            lacing.extend(std::iter::repeat_n(255u8, packet.len() / 255));
            lacing.push((packet.len() % 255) as u8);
        }
        let start = self.out.len();
        self.out.extend_from_slice(b"OggS\0");
        self.out.push(flags);
        self.out.extend_from_slice(&granule.to_le_bytes());
        self.out.extend_from_slice(&0x4865_726Du32.to_le_bytes());
        self.out.extend_from_slice(&self.sequence.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]);
        self.out.push(lacing.len() as u8);
        self.out.extend_from_slice(&lacing);
        for packet in packets {
            self.out.extend_from_slice(packet);
        }
        let crc = crc32(&self.out[start..]);
        self.out[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
        self.sequence += 1;
    }
}

/// Groups packets into pages whose lacing tables fit Ogg's 255-segment count.
/// A packet of `n` bytes takes `n / 255 + 1` lacing entries.
fn page_chunks(packets: &[Vec<u8>]) -> anyhow::Result<Vec<Vec<&[u8]>>> {
    let mut chunks: Vec<Vec<&[u8]>> = Vec::new();
    let mut current: Vec<&[u8]> = Vec::new();
    let mut segments = 0usize;
    for packet in packets {
        let need = packet.len() / 255 + 1;
        if need > 255 {
            anyhow::bail!("an Opus packet is larger than one Ogg page can hold");
        }
        if segments + need > 255 {
            chunks.push(std::mem::take(&mut current));
            segments = 0;
        }
        current.push(packet);
        segments += need;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

/// Converts a WebM/Opus recording to Ogg/Opus. Errors if the file holds no
/// Opus audio, so a non-Opus recording is never mislabeled as Opus.
pub fn webm_to_ogg(webm: &[u8]) -> anyhow::Result<Vec<u8>> {
    let (head, packets) = demux_webm(webm)
        .ok_or_else(|| anyhow::anyhow!("the recording is not a WebM file"))?;
    if packets.is_empty() {
        anyhow::bail!("the recording held no audio");
    }
    let head = head
        .filter(|h| h.starts_with(b"OpusHead"))
        .ok_or_else(|| anyhow::anyhow!("the recording's audio is not Opus"))?;
    let mut tags = b"OpusTags".to_vec();
    let vendor = b"postal";
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());

    let mut ogg = OggWriter { out: Vec::with_capacity(webm.len() + 4096), sequence: 0 };
    ogg.page(&[&head], 0, 0x02);
    ogg.page(&[&tags], 0, 0);
    let mut granule = 0;
    // A page's lacing table holds 255 entries, so packets are packed into
    // pages by their total segment count instead of a fixed packet count.
    let chunks = page_chunks(&packets)?;
    for (i, chunk) in chunks.iter().enumerate() {
        granule += chunk.iter().map(|p| packet_samples(p)).sum::<u64>();
        ogg.page(chunk, granule, if i + 1 == chunks.len() { 0x04 } else { 0 });
    }
    Ok(ogg.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.push(0x80 | body.len() as u8);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn crc_matches_ogg_reference() {
        assert_eq!(crc32(b"123456789"), 0x89A1_897F);
    }

    #[test]
    fn packet_lengths() {
        // CELT 20 ms, one frame.
        assert_eq!(packet_samples(&[0b1111_1000]), 960);
        // SILK 60 ms, two frames.
        assert_eq!(packet_samples(&[(3 << 3) | 1]), 5760);
    }

    #[test]
    fn remuxes_live_webm() {
        let frame = [0b1111_1000u8, 1, 2, 3];
        let mut block = vec![0x81, 0, 0, 0x80];
        block.extend_from_slice(&frame);
        let track = element(&[0xAE], &element(&[0x63, 0xA2], b"OpusHead\x01\x01\x38\x01\x80\xBB\0\0\0\0\0"));
        let mut webm = vec![0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        webm.extend(element(&[0x16, 0x54, 0xAE, 0x6B], &track));
        webm.extend_from_slice(&[0x1F, 0x43, 0xB6, 0x75, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        webm.extend(element(&[0xA3], &block));
        webm.extend(element(&[0xA3], &block));

        let ogg = webm_to_ogg(&webm).expect("remux");
        assert!(ogg.starts_with(b"OggS"));
        assert_eq!(ogg.windows(8).filter(|w| *w == b"OpusHead").count(), 1);
        assert_eq!(ogg.windows(4).filter(|w| *w == b"OggS").count(), 3);
        // The last page ends the stream after two 20 ms packets.
        let last = ogg.windows(4).rposition(|w| w == b"OggS").unwrap();
        assert_eq!(ogg[last + 5], 0x04);
        assert_eq!(u64::from_le_bytes(ogg[last + 6..last + 14].try_into().unwrap()), 1920);
    }

    #[test]
    fn pages_never_exceed_the_lacing_limit() {
        // A 1300-byte packet needs six lacing entries; the old 50-packet
        // chunks needed 300, wrapping the page's single count byte.
        let packets: Vec<Vec<u8>> = (0..100).map(|_| vec![0xF8; 1300]).collect();
        let chunks = page_chunks(&packets).expect("chunks fit");
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 100);
        assert_eq!(chunks.len(), 3);
        for chunk in &chunks {
            let segments: usize = chunk.iter().map(|p| p.len() / 255 + 1).sum();
            assert!(segments <= 255, "a page needs {segments} lacing segments");
        }
    }

    #[test]
    fn rejects_non_opus_webm() {
        let track = element(&[0xAE], &element(&[0x63, 0xA2], b"\x01vorbis"));
        let mut webm = vec![0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        webm.extend(element(&[0x16, 0x54, 0xAE, 0x6B], &track));
        let mut block = vec![0x81, 0, 0, 0x80];
        block.extend_from_slice(&[0xF8, 1, 2, 3]);
        webm.extend(element(&[0xA3], &block));
        assert!(webm_to_ogg(&webm).is_err());
    }
}
