//! IFF ILBM pictures, the Amiga's: a SkyPix brush sent by XMODEM (see
//! [`crate::skypix`]). Bit planes, interleaved a row at a time,
//! uncompressed or ByteRun1, with an optional mask plane; the result is a
//! pen (palette index) per pixel.

fn be16(d: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?) as usize)
}

/// The chunks of an IFF `FORM` of `kind`: (ID, data).
pub fn chunks<'a>(data: &'a [u8], kind: &[u8]) -> Option<Vec<(&'a [u8], &'a [u8])>> {
    if !data.starts_with(b"FORM") || data.get(8..12)? != kind {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 12;
    while at + 8 <= data.len() {
        let id = &data[at..at + 4];
        let len = u32::from_be_bytes(data[at + 4..at + 8].try_into().ok()?) as usize;
        let body = data.get(at + 8..(at + 8 + len).min(data.len()))?;
        out.push((id, body));
        at += 8 + len + (len & 1);
    }
    Some(out)
}

/// ByteRun1 (PackBits): `n` (0-127) literal n+1 bytes, -1..-127 the next
/// byte -n+1 times.
fn unpack(src: &[u8], want: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(want);
    let mut i = 0;
    while out.len() < want && i < src.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let end = (i + n as usize + 1).min(src.len());
            out.extend_from_slice(&src[i..end]);
            i = end;
        } else if n != -128 {
            if let Some(&b) = src.get(i) {
                out.extend(std::iter::repeat_n(b, (-(n as i32) + 1) as usize));
            }
            i += 1;
        }
    }
    out.resize(want, 0);
    out
}

/// An ILBM's (width, height, pens).
pub fn ilbm(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let chunks = chunks(data, b"ILBM")?;
    let bmhd = chunks.iter().find(|(id, _)| *id == b"BMHD")?.1;
    let body = chunks.iter().find(|(id, _)| *id == b"BODY")?.1;
    let (w, h) = (be16(bmhd, 0)?, be16(bmhd, 2)?);
    let (planes, masking, compression) = (*bmhd.get(8)? as usize, *bmhd.get(9)?, *bmhd.get(10)?);
    if w == 0 || h == 0 || w > 4096 || h > 4096 || !(1..=8).contains(&planes) {
        return None;
    }
    let row = w.div_ceil(16) * 2;
    let stored = planes + usize::from(masking == 1);
    let bits = if compression == 1 { unpack(body, row * stored * h) } else { body.to_vec() };
    let mut pens = vec![0u8; w * h];
    for y in 0..h {
        for p in 0..planes {
            let start = (y * stored + p) * row;
            let Some(line) = bits.get(start..start + row) else { break };
            for x in 0..w {
                if line[x / 8] & (0x80 >> (x % 8)) != 0 {
                    pens[y * w + x] |= 1 << p;
                }
            }
        }
    }
    Some((w, h, pens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_planes_compressed_or_not() {
        let form = |compression: u8, body: &[u8]| {
            let mut bmhd = vec![0, 4, 0, 2, 0, 0, 0, 0, 2, 0, compression, 0, 0, 0, 10, 11, 0, 4, 0, 2];
            bmhd.truncate(20);
            let mut f = b"FORM\0\0\0\0ILBM".to_vec();
            for (id, d) in [(b"BMHD", &bmhd[..]), (b"BODY", body)] {
                f.extend(id);
                f.extend((d.len() as u32).to_be_bytes());
                f.extend(d);
                if d.len() % 2 == 1 {
                    f.push(0);
                }
            }
            f
        };
        // 4x2, two planes: row 0 pens 1,2,3,0; row 1 all 3.
        let raw = [0b1010_0000, 0, 0b0110_0000, 0, 0xF0, 0, 0xF0, 0];
        let want = vec![1, 2, 3, 0, 3, 3, 3, 3];
        assert_eq!(ilbm(&form(0, &raw)), Some((4, 2, want.clone())));
        // ByteRun1: literal runs; and a byte repeated.
        let packed = [3, 0b1010_0000, 0, 0b0110_0000, 0, 1, 0xF0, 0, 1, 0xF0, 0];
        assert_eq!(ilbm(&form(1, &packed)), Some((4, 2, want)));
        assert_eq!(unpack(&[(-2i8) as u8, 7, 0, 9], 4), [7, 7, 7, 9]);
        assert!(ilbm(b"FORM\0\0\0\x04ILBM").is_none());
    }
}
