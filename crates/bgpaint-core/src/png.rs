//! A PNG encoder that uses every core: rows are converted from premultiplied pixels and filtered
//! in strips, each strip is deflated on its own and the pieces are joined into one zlib stream
//! (like pigz). The output is a standard PNG; the image is never copied whole.
//!
//! The first row of each strip never refers to the row above it, and a private ancillary chunk
//! (`bgPS`, ignored by other programs and dropped by editors that change the image) records the
//! strips, so [`decode`] can also inflate and unfilter them on every core.

use crate::blend::Px;
use flate2::{Compress, Compression, FlushCompress, Status};
use rayon::prelude::*;
use tiny_skia::Pixmap;

/// The chunk listing the strips.
const INDEX: &[u8; 4] = b"bgPS";
/// Rows per independently compressed strip.
const STRIP: usize = 128;
/// Largest IDAT chunk written.
const CHUNK: usize = 1 << 20;

/// Encodes `p` as an 8-bit PNG: RGB when it is fully opaque, else straight-alpha RGBA.
/// `level` is the deflate level (0–9).
pub fn encode(p: &Pixmap, level: u32) -> Vec<u8> {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let (data, _) = p.data().as_chunks::<4>();
    let opaque = data.par_chunks(1 << 16).all(|c| c.iter().all(|px| px[3] == 255));
    encode_rows(w, h, opaque, level, |y| &data[y * w..(y + 1) * w])
}

/// A `w`×`h` PNG filled with one premultiplied color, without building the image.
pub fn encode_solid(w: u32, h: u32, color: Px, level: u32) -> Vec<u8> {
    let row = vec![color; w as usize];
    encode_rows(w as usize, h as usize, color[3] == 255, level, |_| &row)
}

/// Encodes `h` rows of `w` premultiplied pixels given by `row(y)`.
fn encode_rows<'a>(w: usize, h: usize, opaque: bool, level: u32, row: impl Fn(usize) -> &'a [Px] + Sync) -> Vec<u8> {
    let bpp = if opaque { 3 } else { 4 };
    let row_len = w * bpp;
    let strips: Vec<(Vec<u8>, u32, usize)> = (0..h.div_ceil(STRIP))
        .into_par_iter()
        .map(|s| {
            let (r0, r1) = (s * STRIP, ((s + 1) * STRIP).min(h));
            let mut prev = vec![0u8; row_len];
            if r0 > 0 {
                convert(row(r0 - 1), opaque, &mut prev);
            }
            let mut cur = vec![0u8; row_len];
            let mut filtered = Vec::with_capacity((r1 - r0) * (row_len + 1));
            let mut scratch = vec![0u8; row_len];
            for y in r0..r1 {
                convert(row(y), opaque, &mut cur);
                filter_row(&cur, &prev, bpp, y == r0, &mut scratch, &mut filtered);
                std::mem::swap(&mut prev, &mut cur);
            }
            let adler = adler32(&filtered);
            (deflate(&filtered, level, r1 == h), adler, filtered.len())
        })
        .collect();

    let mut zlib = Vec::with_capacity(strips.iter().map(|s| s.0.len()).sum::<usize>() + 6);
    zlib.extend_from_slice(&[0x78, 0x9c]);
    let mut adler = 1u32;
    for (bytes, a, len) in &strips {
        zlib.extend_from_slice(bytes);
        adler = adler32_combine(adler, *a, *len);
    }
    zlib.extend_from_slice(&adler.to_be_bytes());

    let mut out = Vec::with_capacity(zlib.len() + 64 + zlib.len() / CHUNK * 12);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, if opaque { 2 } else { 6 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let mut index = Vec::with_capacity(4 + strips.len() * 4);
    index.extend_from_slice(&(STRIP as u32).to_be_bytes());
    for (bytes, _, _) in &strips {
        index.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    }
    chunk(&mut out, INDEX, &index);
    for part in zlib.chunks(CHUNK) {
        chunk(&mut out, b"IDAT", part);
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Straight-alpha bytes of a row of premultiplied pixels (RGB only when `opaque`).
fn convert(row: &[Px], opaque: bool, out: &mut [u8]) {
    if opaque {
        for (o, p) in out.as_chunks_mut::<3>().0.iter_mut().zip(row) {
            o.copy_from_slice(&p[..3]);
        }
        return;
    }
    for (o, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(row) {
        o.copy_from_slice(&crate::raster::demultiply(*p));
    }
}

/// Appends `cur` filtered with whichever PNG filter gives the smallest sum of absolute values
/// (the usual adaptive heuristic), preceded by the filter type.
/// The `first` row of a strip only uses the filters that ignore the row above.
fn filter_row(cur: &[u8], prev: &[u8], bpp: usize, first: bool, scratch: &mut [u8], out: &mut Vec<u8>) {
    // A row like the one above (flat areas, empty layers) is all zeros with the Up filter.
    if cur == prev && !first {
        out.push(2);
        out.resize(out.len() + cur.len(), 0);
        return;
    }
    // The cost of the five filters in one pass.
    let mut cost = [0u64; 5];
    for i in 0..cur.len() {
        let (a, b, c) = predictors(cur, prev, bpp, i);
        let x = cur[i];
        let abs = |v: u8| u64::from((v as i8).unsigned_abs());
        cost[0] += abs(x);
        cost[1] += abs(x.wrapping_sub(a));
        cost[2] += abs(x.wrapping_sub(b));
        cost[3] += abs(x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8));
        cost[4] += abs(x.wrapping_sub(paeth(a, b, c)));
    }
    let best = (0..if first { 2 } else { 5 }).min_by_key(|&f| cost[f]).unwrap_or(0);
    for (i, s) in scratch.iter_mut().enumerate() {
        let (a, b, c) = predictors(cur, prev, bpp, i);
        let p = match best {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            _ => paeth(a, b, c),
        };
        *s = cur[i].wrapping_sub(p);
    }
    out.push(best as u8);
    out.extend_from_slice(scratch);
}

/// The left, up and up-left neighbours of byte `i`.
#[inline]
fn predictors(cur: &[u8], prev: &[u8], bpp: usize, i: usize) -> (u8, u8, u8) {
    if i >= bpp { (cur[i - bpp], prev[i], prev[i - bpp]) } else { (0, prev[i], 0) }
}

#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = ((p - i16::from(a)).abs(), (p - i16::from(b)).abs(), (p - i16::from(c)).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decodes a PNG written by [`encode`] on every core. `None` for any other PNG (or a damaged
/// one), which the usual decoder then handles.
pub fn decode(bytes: &[u8]) -> Option<Pixmap> {
    let mut pos = 8;
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let (mut ihdr, mut index, mut idat) = (None, None, Vec::new());
    loop {
        let len = u32::from_be_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?) as usize;
        let kind: [u8; 4] = bytes.get(pos + 4..pos + 8)?.try_into().ok()?;
        let data = bytes.get(pos + 8..pos + 8 + len)?;
        let crc = u32::from_be_bytes(bytes.get(pos + 8 + len..pos + 12 + len)?.try_into().ok()?);
        let mut h = crc32fast::Hasher::new();
        h.update(&kind);
        h.update(data);
        if h.finalize() != crc {
            return None;
        }
        match &kind {
            b"IHDR" => ihdr = Some(data),
            b"IDAT" => idat.push(data),
            b"IEND" => break,
            k if k == INDEX => index = Some(data),
            // Anything else (gamma, color profiles...) is left to the full decoder.
            _ => return None,
        }
        pos += 12 + len;
    }
    let ihdr = ihdr?;
    let index = index?;
    if ihdr.len() != 13 || ihdr[8] != 8 || ihdr[10..13] != [0, 0, 0] {
        return None;
    }
    let w = u32::from_be_bytes(ihdr[0..4].try_into().ok()?);
    let h = u32::from_be_bytes(ihdr[4..8].try_into().ok()?);
    let bpp = match ihdr[9] {
        2 => 3,
        6 => 4,
        _ => return None,
    };
    let mut out = crate::raster::try_pixmap(w, h).ok()?;
    let rows_per = u32::from_be_bytes(index.get(..4)?.try_into().ok()?) as usize;
    let lens: Vec<usize> = index[4..].as_chunks::<4>().0.iter().map(|c| u32::from_be_bytes(*c) as usize).collect();
    let (w, h) = (w as usize, h as usize);
    if rows_per == 0 || lens.len() != h.div_ceil(rows_per) {
        return None;
    }
    // The zlib stream: a 2-byte header, the strips, the checksum.
    let zlib: Vec<u8> = idat.concat();
    let mut starts = Vec::with_capacity(lens.len());
    let mut at = 2;
    for len in &lens {
        starts.push(at);
        at += len;
    }
    if at + 4 != zlib.len() {
        return None;
    }
    let row_len = w * bpp;
    let (pixels, _) = out.data_mut().as_chunks_mut::<4>();
    let ok = pixels.par_chunks_mut(w * rows_per).enumerate().all(|(s, dst)| {
        let rows = dst.len() / w;
        let Some(raw) = inflate(&zlib[starts[s]..starts[s] + lens[s]], rows * (row_len + 1)) else { return false };
        let mut prev = vec![0u8; row_len];
        let mut cur = vec![0u8; row_len];
        for (r, line) in raw.chunks_exact(row_len + 1).enumerate() {
            let f = line[0];
            if f > 4 || (r == 0 && f > 1) {
                return false;
            }
            unfilter(f, &line[1..], &prev, bpp, &mut cur);
            let out_row = &mut dst[r * w..(r + 1) * w];
            if bpp == 3 {
                for (o, p) in out_row.iter_mut().zip(cur.as_chunks::<3>().0) {
                    *o = [p[0], p[1], p[2], 255];
                }
            } else {
                for (o, p) in out_row.iter_mut().zip(cur.as_chunks::<4>().0) {
                    *o = crate::color::Color::rgba(p[0], p[1], p[2], p[3]).premultiplied();
                }
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        true
    });
    ok.then_some(out)
}

/// Inflates a raw deflate piece that must give exactly `size` bytes.
fn inflate(data: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut d = flate2::Decompress::new(false);
    let mut out = Vec::with_capacity(size);
    d.decompress_vec(data, &mut out, flate2::FlushDecompress::Sync).ok()?;
    (out.len() == size && d.total_in() as usize == data.len()).then_some(out)
}

fn unfilter(f: u8, line: &[u8], prev: &[u8], bpp: usize, cur: &mut [u8]) {
    for i in 0..line.len() {
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let (b, c) = (prev[i], if i >= bpp { prev[i - bpp] } else { 0 });
        let p = match f {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            _ => paeth(a, b, c),
        };
        cur[i] = line[i].wrapping_add(p);
    }
}

/// Raw deflate of `data`, ending on a byte boundary (sync flush) unless it is the `last` piece.
fn deflate(data: &[u8], level: u32, last: bool) -> Vec<u8> {
    let mut c = Compress::new(Compression::new(level), false);
    let mut out = Vec::with_capacity(data.len() / 2 + 1024);
    let flush = if last { FlushCompress::Finish } else { FlushCompress::Sync };
    loop {
        if out.capacity() - out.len() < 1 << 16 {
            out.reserve(1 << 20);
        }
        let status = c.compress_vec(&data[c.total_in() as usize..], &mut out, flush).expect("deflate never fails on memory");
        let done = c.total_in() as usize == data.len() && out.len() < out.capacity();
        if done && (!last || status == Status::StreamEnd) {
            return out;
        }
    }
}

const MOD: u32 = 65_521;

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for block in data.chunks(5552) {
        for &x in block {
            a += u32::from(x);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// Adler-32 of two pieces joined, from their checksums and the second one's length.
fn adler32_combine(a1: u32, a2: u32, len2: usize) -> u32 {
    let rem = (len2 % MOD as usize) as u64;
    let m = u64::from(MOD);
    let (s1a, s2a) = (u64::from(a1 & 0xffff), u64::from(a1 >> 16));
    let (s1b, s2b) = (u64::from(a2 & 0xffff), u64::from(a2 >> 16));
    let s1 = (s1a + s1b + m - 1) % m;
    let s2 = (s2a + s2b + rem * s1a % m + m - rem) % m;
    ((s2 as u32) << 16) | s1 as u32
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster;
    use image::ImageEncoder;

    fn image(w: u32, h: u32, alpha: bool) -> Pixmap {
        let mut p = raster::pixmap(w, h);
        for (i, px) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = (i as u32).wrapping_mul(2_654_435_761) >> 8;
            let a = if alpha { [0, 255, (v >> 16) as u8][(v % 3) as usize] } else { 255 };
            let c = |s: u32| (((v >> s) & 0xff) as u8).min(a);
            *px = if i % 7 < 3 { [10, 20, 30, 255].map(|x: u8| x.min(a)) } else { [c(0), c(8), c(4), a] };
        }
        p
    }

    #[test]
    fn decodes_to_the_same_pixels() {
        for alpha in [false, true] {
            for (w, h) in [(1, 1), (3, 300), (517, 261)] {
                let p = image(w, h, alpha);
                let bytes = encode(&p, 6);
                let back = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap().into_rgba8();
                assert_eq!((back.width(), back.height()), (w, h));
                let expected = raster::to_straight_rgba(&p);
                assert!(back.as_raw() == &expected, "{w}×{h} alpha {alpha}");
            }
        }
    }

    #[test]
    fn own_files_decode_in_parallel_like_any_decoder() {
        for alpha in [false, true] {
            let p = image(700, 333, alpha);
            let bytes = encode(&p, 6);
            let fast = decode(&bytes).expect("our own PNG");
            let slow = crate::io::decode(&bytes, "png").unwrap();
            assert!(fast.data() == slow.data());
            let expected = raster::from_straight_rgba(700, 333, &raster::to_straight_rgba(&p)).unwrap();
            assert!(fast.data() == expected.data());
        }
        // Another program's PNG is left to the usual decoder.
        let mut other = Vec::new();
        image::codecs::png::PngEncoder::new(&mut other).write_image(&[1, 2, 3, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        assert!(decode(&other).is_none());
    }

    #[test]
    fn adler_pieces_combine() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        let (a, b) = data.split_at(33_333);
        assert_eq!(adler32_combine(adler32(a), adler32(b), b.len()), adler32(&data));
    }
}
