//! Color definitions: the 16-color CGA/VGA palette plus the xterm 256-color
//! extension and 24-bit RGB used by modern BBS software.

/// A packed 0x00RRGGBB color, the native format of the frame buffer.
pub type Rgb = u32;

/// Canonical VGA text-mode palette in attribute order (blue, green, cyan,
/// red, ...), with the CGA "brown" fix for index 6.
#[rustfmt::skip]
pub const VGA: [Rgb; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA,
    0x555555, 0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// Map an ANSI color number (SGR 30-37 order: black, red, green, yellow,
/// blue, magenta, cyan, white) to its VGA palette index.
///
/// ANSI and the PC attribute byte order the RGB bits differently, so red and
/// blue are swapped here.
pub const ANSI_TO_VGA: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// Resolve an xterm 256-color index to RGB. Indices 0-15 are the ANSI colors
/// (in ANSI order) rendered with the VGA palette.
pub fn xterm256(i: u8) -> Rgb {
    match i {
        0..=7 => VGA[ANSI_TO_VGA[i as usize] as usize],
        8..=15 => VGA[ANSI_TO_VGA[(i - 8) as usize] as usize + 8],
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            (level(i / 36) << 16) | (level((i / 6) % 6) << 8) | level(i % 6)
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232) as u32;
            (v << 16) | (v << 8) | v
        }
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_cube_corners() {
        assert_eq!(xterm256(16), 0x000000);
        assert_eq!(xterm256(231), 0xFFFFFF);
        assert_eq!(xterm256(196), 0xFF0000);
        assert_eq!(xterm256(1), 0xAA0000);
        assert_eq!(xterm256(4), 0x0000AA);
    }
}
