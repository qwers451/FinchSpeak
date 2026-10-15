//! Fast RGBA/BGRA -> I420 conversion for the video encoder.
//!
//! The converter in the `openh264` crate takes ~14 ms for a 1080p frame, which on its own is
//! almost half of a 30 fps frame budget. This one uses integer maths (BT.601, limited range) and
//! splits the picture into bands converted on scoped threads.

use std::thread;

/// Byte order of a packed 32-bit pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelOrder {
    Rgba,
    Bgra,
}

/// Bands below this many pixels are not worth a thread.
const MIN_PIXELS_PER_THREAD: usize = 256 * 1024;
const MAX_THREADS: usize = 4;

/// Convert a `width`x`height` packed frame into I420 (`Y`, then `U`, then `V`) in `out`.
///
/// `width` and `height` must be even and `packed` must hold `width * height * 4` bytes.
pub fn packed_to_i420(packed: &[u8], width: usize, height: usize, order: PixelOrder, out: &mut Vec<u8>) {
    let (y_len, uv_len) = (width * height, (width / 2) * (height / 2));
    out.clear();
    out.resize(y_len + 2 * uv_len, 0);
    if width == 0 || height == 0 || packed.len() < y_len * 4 {
        return;
    }
    let (y_plane, chroma) = out.split_at_mut(y_len);
    let (u_plane, v_plane) = chroma.split_at_mut(uv_len);

    // Bands are whole pairs of rows, so every chroma sample has all four of its pixels in one band.
    let pairs = height / 2;
    let threads = (y_len / MIN_PIXELS_PER_THREAD).clamp(1, MAX_THREADS).min(pairs.max(1));
    let pairs_per_band = pairs.div_ceil(threads);

    thread::scope(|scope| {
        let (mut y_rest, mut u_rest, mut v_rest) = (y_plane, u_plane, v_plane);
        let mut first_pair = 0;
        while first_pair < pairs {
            let band_pairs = pairs_per_band.min(pairs - first_pair);
            let (y_band, y_next) = y_rest.split_at_mut(band_pairs * 2 * width);
            let (u_band, u_next) = u_rest.split_at_mut(band_pairs * (width / 2));
            let (v_band, v_next) = v_rest.split_at_mut(band_pairs * (width / 2));
            (y_rest, u_rest, v_rest) = (y_next, u_next, v_next);
            let src = &packed[first_pair * 2 * width * 4..(first_pair + band_pairs) * 2 * width * 4];
            scope.spawn(move || convert_band(src, width, order, y_band, u_band, v_band));
            first_pair += band_pairs;
        }
    });
}

fn convert_band(src: &[u8], width: usize, order: PixelOrder, y: &mut [u8], u: &mut [u8], v: &mut [u8]) {
    let (ri, bi) = match order {
        PixelOrder::Rgba => (0, 2),
        PixelOrder::Bgra => (2, 0),
    };
    let row_bytes = width * 4;
    for (pair, (u_row, v_row)) in u.chunks_exact_mut(width / 2).zip(v.chunks_exact_mut(width / 2)).enumerate() {
        let top = &src[pair * 2 * row_bytes..pair * 2 * row_bytes + row_bytes];
        let bottom = &src[(pair * 2 + 1) * row_bytes..(pair * 2 + 2) * row_bytes];
        let (y_top, y_bottom) = y[pair * 2 * width..(pair * 2 + 2) * width].split_at_mut(width);
        for x in 0..width / 2 {
            let (mut r_sum, mut g_sum, mut b_sum) = (0i32, 0i32, 0i32);
            for (row, y_row) in [(top, &mut *y_top), (bottom, &mut *y_bottom)] {
                for dx in 0..2 {
                    let px = &row[(x * 2 + dx) * 4..(x * 2 + dx) * 4 + 4];
                    let (r, g, b) = (i32::from(px[ri]), i32::from(px[1]), i32::from(px[bi]));
                    y_row[x * 2 + dx] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
                    r_sum += r;
                    g_sum += g;
                    b_sum += b;
                }
            }
            let (r, g, b) = ((r_sum + 2) >> 2, (g_sum + 2) >> 2, (b_sum + 2) >> 2);
            u_row[x] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            v_row[x] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::formats::{RgbaSliceU8, YUVBuffer, YUVSource};

    /// A gradient with noise, big enough to be split across threads.
    fn picture(width: usize, height: usize) -> Vec<u8> {
        let mut state = 7u32;
        (0..width * height * 4)
            .map(|i| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (state >> 28) as u8;
                ((i / 4 % width * 255 / width) as u8).wrapping_add(noise).wrapping_add((i % 4) as u8 * 30)
            })
            .collect()
    }

    fn max_diff(a: &[u8], b: &[u8]) -> u8 {
        a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
    }

    #[test]
    fn matches_the_reference_converter() {
        for (w, h) in [(64, 48), (1920, 1080), (322, 182)] {
            let rgba = picture(w, h);
            let mut ours = Vec::new();
            packed_to_i420(&rgba, w, h, PixelOrder::Rgba, &mut ours);

            let reference = YUVBuffer::from_rgb_source(RgbaSliceU8::new(&rgba, (w, h)));
            let (y_len, uv_len) = (w * h, (w / 2) * (h / 2));
            assert!(max_diff(&ours[..y_len], reference.y()) <= 2, "luma differs at {w}x{h}");
            assert!(max_diff(&ours[y_len..y_len + uv_len], reference.u()) <= 4, "U differs at {w}x{h}");
            assert!(max_diff(&ours[y_len + uv_len..], reference.v()) <= 4, "V differs at {w}x{h}");
        }
    }

    #[test]
    fn bgra_gives_the_same_picture_as_rgba() {
        let (w, h) = (640, 360);
        let rgba = picture(w, h);
        let mut bgra = rgba.clone();
        for px in bgra.as_chunks_mut::<4>().0.iter_mut() {
            px.swap(0, 2);
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        packed_to_i420(&rgba, w, h, PixelOrder::Rgba, &mut a);
        packed_to_i420(&bgra, w, h, PixelOrder::Bgra, &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn primary_colours_land_where_expected() {
        let (w, h) = (4, 4);
        let red: Vec<u8> = [255, 0, 0, 255].repeat(w * h);
        let mut out = Vec::new();
        packed_to_i420(&red, w, h, PixelOrder::Rgba, &mut out);
        assert_eq!(out[0], 82, "Y of pure red (BT.601 limited range)");
        assert_eq!(out[w * h], 90, "U of pure red");
        assert_eq!(out[w * h + w * h / 4], 240, "V of pure red");
    }
}
