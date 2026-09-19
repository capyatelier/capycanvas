#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
use alloc::vec;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use super::primitives;
use crate::huffman::{MAX_BITS, MAX_SYMBOL_VALUE};

#[derive(Clone)]
pub struct HuffmanEncodeTable {
    codes: [u16; MAX_SYMBOL_VALUE + 1],
    num_bits: [u8; MAX_SYMBOL_VALUE + 1],
    weights: Vec<u8>,
    max_symbol: u8,
    table_log: u8,
    serialized_weights: Vec<u8>,
}

#[cfg(feature = "alloc")]
impl HuffmanEncodeTable {
    pub fn from_data(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let mut freqs = [0u32; MAX_SYMBOL_VALUE + 1];
        let mut max_sym = 0u8;
        for &b in data {
            freqs[b as usize] += 1;
            if b > max_sym {
                max_sym = b;
            }
        }

        let num_symbols = max_sym as usize + 1;
        let active_count = freqs[..num_symbols].iter().filter(|&&f| f > 0).count();
        if active_count < 2 {
            return None;
        }

        let (weights, table_log) = compute_huffman_weights(&freqs, num_symbols)?;
        let (codes, num_bits) = build_encode_codes(&weights, table_log);

        let mut table = Self {
            codes,
            num_bits,
            weights,
            max_symbol: max_sym,
            table_log,
            serialized_weights: Vec::new(),
        };
        table.serialized_weights = table.try_serialize_weights()?;
        Some(table)
    }

    pub fn from_decode_table(
        decode_table: &[super::HuffmanDecodeEntry],
        table_log: u8,
    ) -> Option<Self> {
        let table_size = 1usize << table_log;
        if decode_table.len() < table_size {
            return None;
        }

        let mut num_bits_per_sym = [0u8; MAX_SYMBOL_VALUE + 1];
        let mut max_sym = 0u8;
        let mut seen = [false; MAX_SYMBOL_VALUE + 1];
        for entry in &decode_table[..table_size] {
            let s = entry.symbol;
            if !seen[s as usize] {
                num_bits_per_sym[s as usize] = entry.num_bits;
                seen[s as usize] = true;
                if s > max_sym {
                    max_sym = s;
                }
            }
        }

        let num_symbols = max_sym as usize + 1;
        let active_count = seen[..num_symbols].iter().filter(|&&s| s).count();
        if active_count < 2 {
            return None;
        }

        let mut weights = vec![0u8; num_symbols];
        for s in 0..num_symbols {
            if seen[s] {
                weights[s] = table_log + 1 - num_bits_per_sym[s];
            }
        }

        let (codes, num_bits) = build_encode_codes(&weights, table_log);

        let mut table = Self {
            codes,
            num_bits,
            weights,
            max_symbol: max_sym,
            table_log,
            serialized_weights: Vec::new(),
        };
        table.serialized_weights = table.try_serialize_weights()?;
        Some(table)
    }

    pub fn table_log(&self) -> u8 {
        self.table_log
    }

    pub fn can_encode(&self, data: &[u8]) -> bool {
        for &b in data {
            if self.num_bits[b as usize] == 0 {
                return false;
            }
        }
        true
    }

    pub fn serialize_weights(&self) -> Vec<u8> {
        self.serialized_weights.clone()
    }

    fn try_serialize_weights(&self) -> Option<Vec<u8>> {
        let explicit = &self.weights[..self.max_symbol as usize];
        let num_symbols = explicit.len();

        if num_symbols > 128 {
            return compress_weights(explicit);
        }
        let mut out = Vec::with_capacity(1 + num_symbols.div_ceil(2));
        out.push((num_symbols + 127) as u8);
        let num_bytes = num_symbols.div_ceil(2);
        for i in 0..num_bytes {
            let hi = explicit.get(i * 2).copied().unwrap_or(0);
            let lo = explicit.get(i * 2 + 1).copied().unwrap_or(0);
            out.push((hi << 4) | lo);
        }
        Some(out)
    }

    pub fn encode_single_stream(&self, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(data.len() + 8);
        self.encode_single_stream_into(data, &mut buf);
        buf
    }

    pub fn encode_single_stream_into(&self, data: &[u8], buf: &mut Vec<u8>) {
        let tl = self.table_log as usize;
        let unroll: usize = (32usize).checked_div(tl).unwrap_or(1).max(2);

        let mut bitstream = primitives::BitstreamScratch::new(buf, data.len() + 16);
        let mut bits: u64 = 0;
        let mut bits_used: u8 = 0;
        let mut wpos: usize = 0;

        macro_rules! flush_bits {
            () => {
                bitstream.flush(wpos, bits);
                let nb = (bits_used >> 3) as usize;
                wpos += nb;
                bits >>= nb << 3;
                bits_used &= 7;
            };
        }

        let mut pos = data.len();
        while pos >= unroll {
            pos -= unroll;
            for j in 0..unroll {
                let b = data[pos + (unroll - 1 - j)];
                let c = self.codes[b as usize] as u64;
                let n = self.num_bits[b as usize];
                bits |= c << bits_used;
                bits_used += n;
            }
            if bits_used >= 32 {
                flush_bits!();
            }
        }
        while pos > 0 {
            pos -= 1;
            let b = data[pos];
            let c = self.codes[b as usize] as u64;
            let n = self.num_bits[b as usize];
            bits |= c << bits_used;
            bits_used += n;
            if bits_used >= 32 {
                flush_bits!();
            }
        }

        bits |= 1u64 << bits_used;
        bits_used += 1;
        while bits_used > 0 {
            bitstream.write_byte(wpos, bits as u8);
            wpos += 1;
            bits >>= 8;
            bits_used = bits_used.saturating_sub(8);
        }
        bitstream.finish(wpos);
    }

    pub fn encode_4_streams(&self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_4_streams_into(data, &mut out, &mut Vec::new());
        out
    }

    pub fn encode_4_streams_into(&self, data: &[u8], out: &mut Vec<u8>, stream_buf: &mut Vec<u8>) {
        let seg = data.len().div_ceil(4);
        let s1 = &data[..seg.min(data.len())];
        let s2 = &data[seg.min(data.len())..(seg * 2).min(data.len())];
        let s3 = &data[(seg * 2).min(data.len())..(seg * 3).min(data.len())];
        let s4 = &data[(seg * 3).min(data.len())..];

        out.clear();
        out.extend_from_slice(&[0u8; 6]);

        self.encode_single_stream_into(s1, stream_buf);
        let e1_len = stream_buf.len();
        out.extend_from_slice(stream_buf);

        self.encode_single_stream_into(s2, stream_buf);
        let e2_len = stream_buf.len();
        out.extend_from_slice(stream_buf);

        self.encode_single_stream_into(s3, stream_buf);
        let e3_len = stream_buf.len();
        out.extend_from_slice(stream_buf);

        self.encode_single_stream_into(s4, stream_buf);
        out.extend_from_slice(stream_buf);

        out[0..2].copy_from_slice(&(e1_len as u16).to_le_bytes());
        out[2..4].copy_from_slice(&(e2_len as u16).to_le_bytes());
        out[4..6].copy_from_slice(&(e3_len as u16).to_le_bytes());
    }

    pub fn compressed_size_single(&self, data: &[u8]) -> usize {
        let total_bits: usize = data
            .iter()
            .map(|&b| self.num_bits[b as usize] as usize)
            .sum();
        (total_bits + 8) / 8
    }
}

fn compute_huffman_weights(freqs: &[u32], num_symbols: usize) -> Option<(Vec<u8>, u8)> {
    use alloc::collections::BinaryHeap;
    use core::cmp::Reverse;

    let active: Vec<(u64, usize)> = freqs[..num_symbols]
        .iter()
        .enumerate()
        .filter(|(_, f)| **f > 0)
        .map(|(s, &f)| (f as u64, s))
        .collect();

    if active.len() < 2 {
        return None;
    }

    let n = active.len();

    let max_nodes = 2 * n;
    let mut parent = vec![usize::MAX; max_nodes];

    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::with_capacity(n);
    for (i, &(f, _)) in active.iter().enumerate() {
        heap.push(Reverse((f, i)));
    }

    for next_id in n..n + (n - 1) {
        let Reverse((f1, n1)) = heap.pop().unwrap();
        let Reverse((f2, n2)) = heap.pop().unwrap();
        parent[n1] = next_id;
        parent[n2] = next_id;
        heap.push(Reverse((f1 + f2, next_id)));
    }

    let mut bit_lengths = vec![0u8; num_symbols];
    for (i, &(_, sym)) in active.iter().enumerate().take(n) {
        let mut depth = 0u8;
        let mut node = i;
        while parent[node] != usize::MAX {
            depth += 1;
            node = parent[node];
        }
        bit_lengths[sym] = depth;
    }

    let mut max_bl = *bit_lengths.iter().max().unwrap();
    if max_bl == 0 {
        return None;
    }
    if max_bl > MAX_BITS {
        let mut lengths = vec![0usize; max_bl as usize + 1];
        for &len in &bit_lengths {
            if len > 0 {
                lengths[len as usize] += 1;
            }
        }
        for i in (MAX_BITS as usize + 1..lengths.len()).rev() {
            while lengths[i] > 0 {
                let j = (1..i - 1).rev().find(|&j| lengths[j] > 0)?;
                lengths[i] -= 2;
                lengths[i - 1] += 1;
                lengths[j] -= 1;
                lengths[j + 1] += 2;
            }
        }
        let mut ordered = active.clone();
        ordered.sort_unstable_by(|a, b| b.cmp(a));
        let mut at = 0;
        for (len, &count) in lengths
            .iter()
            .enumerate()
            .take(MAX_BITS as usize + 1)
            .skip(1)
        {
            for _ in 0..count {
                bit_lengths[ordered[at].1] = len as u8;
                at += 1;
            }
        }
        max_bl = MAX_BITS;
    }
    let table_log = max_bl;
    let mut weights = vec![0u8; num_symbols];
    for (s, &bl) in bit_lengths.iter().enumerate() {
        if bl > 0 {
            weights[s] = table_log + 1 - bl;
        }
    }

    Some((weights, table_log))
}

fn build_encode_codes(
    weights: &[u8],
    table_log: u8,
) -> ([u16; MAX_SYMBOL_VALUE + 1], [u8; MAX_SYMBOL_VALUE + 1]) {
    let mut codes = [0u16; MAX_SYMBOL_VALUE + 1];
    let mut num_bits = [0u8; MAX_SYMBOL_VALUE + 1];

    let max_w = table_log + 1;
    let mut rank_count = [0u32; MAX_BITS as usize + 2];

    for (s, &w) in weights.iter().enumerate() {
        if w > 0 && w <= max_w {
            num_bits[s] = table_log + 1 - w;
            rank_count[w as usize] += 1;
        }
    }

    let mut rank_start = [0u32; MAX_BITS as usize + 2];
    let mut cumul = 0u32;
    for w in 1..=max_w {
        rank_start[w as usize] = cumul;
        cumul += rank_count[w as usize] * (1u32 << (w - 1));
    }

    for (s, &w) in weights.iter().enumerate() {
        if w == 0 {
            continue;
        }
        let start = rank_start[w as usize];
        codes[s] = (start >> (w - 1)) as u16;
        rank_start[w as usize] += 1u32 << (w - 1);
    }

    (codes, num_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_symbol_range_and_limited_skew_preserve_both_weight_encodings() {
        for symbols in 129..=256 {
            for heavy in [0usize, 100_000] {
                let mut data: Vec<_> = (0..symbols).map(|s| s as u8).cycle().take(4096).collect();
                data.extend(core::iter::repeat_n(255, heavy));
                let table = HuffmanEncodeTable::from_data(&data).unwrap();
                assert!(table.table_log <= MAX_BITS);
                let description = table.serialize_weights();
                assert_eq!(description[0] < 128, table.max_symbol > 128);
                let (weights, consumed) =
                    crate::huffman::weights::parse_huffman_weights(&description).unwrap();
                assert_eq!(consumed, description.len());
                // The serialized description omits the final weight; the
                // decode-table builder infers it from the remaining capacity.
                assert_eq!(
                    weights,
                    table.weights[..table.max_symbol as usize],
                    "symbols={symbols} heavy={heavy} header={description:?}"
                );
                let (decode, log) =
                    crate::huffman::weights::build_huffman_decode_table(&weights).unwrap();
                let stream = table.encode_single_stream(&data);
                assert_eq!(
                    crate::huffman::decode::decode_single_stream(&decode, log, &stream, data.len())
                        .unwrap(),
                    data
                );
            }
        }
    }

    #[test]
    fn from_decode_table_roundtrip() {
        let data = b"hello world hello world hello world!";
        let original = HuffmanEncodeTable::from_data(data).unwrap();
        let weights_raw = original.serialize_weights();

        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();

        let rebuilt = HuffmanEncodeTable::from_decode_table(&decode_table, decode_log).unwrap();
        assert_eq!(original.table_log, rebuilt.table_log);
        assert_eq!(original.max_symbol, rebuilt.max_symbol);
        assert_eq!(original.weights, rebuilt.weights);
        assert_eq!(original.num_bits, rebuilt.num_bits);
        assert_eq!(original.codes, rebuilt.codes);

        let encoded = rebuilt.encode_single_stream(data);
        let decoded = crate::huffman::decode::decode_single_stream(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn from_decode_table_skewed() {
        let mut data = vec![0u8; 900];
        data.extend(vec![1u8; 80]);
        data.extend(vec![2u8; 15]);
        data.extend(vec![3u8; 5]);
        let original = HuffmanEncodeTable::from_data(&data).unwrap();
        let weights_raw = original.serialize_weights();

        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();

        let rebuilt = HuffmanEncodeTable::from_decode_table(&decode_table, decode_log).unwrap();
        assert_eq!(original.codes, rebuilt.codes);
        assert_eq!(original.num_bits, rebuilt.num_bits);

        let encoded = rebuilt.encode_single_stream(&data);
        let decoded = crate::huffman::decode::decode_single_stream(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_simple() {
        let data = b"hello world hello world hello world!";
        let table = HuffmanEncodeTable::from_data(data).unwrap();
        let weights_raw = table.serialize_weights();
        let encoded = table.encode_single_stream(data);

        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();
        let decoded = crate::huffman::decode::decode_single_stream(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_4_streams() {
        let data: Vec<u8> = b"ABCDEFGH".iter().cycle().take(1024).copied().collect();
        let table = HuffmanEncodeTable::from_data(&data).unwrap();
        let weights_raw = table.serialize_weights();
        let encoded = table.encode_4_streams(&data);

        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();
        let decoded = crate::huffman::decode::decode_4_streams(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_all_bytes() {
        let data: Vec<u8> = (0u8..=127).cycle().take(4096).collect();
        let table = HuffmanEncodeTable::from_data(&data).unwrap();
        let weights_raw = table.serialize_weights();
        let encoded = table.encode_single_stream(&data);

        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();
        let decoded = crate::huffman::decode::decode_single_stream(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn skewed_distribution() {
        let mut data = vec![0u8; 900];
        data.extend(vec![1u8; 80]);
        data.extend(vec![2u8; 15]);
        data.extend(vec![3u8; 5]);
        let table = HuffmanEncodeTable::from_data(&data).unwrap();
        assert!(table.num_bits[0] < table.num_bits[3]);
        let weights_raw = table.serialize_weights();
        let encoded = table.encode_single_stream(&data);

        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();
        let decoded = crate::huffman::decode::decode_single_stream(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn two_symbols() {
        let mut data = vec![0u8; 500];
        data.extend(vec![1u8; 500]);
        let table = HuffmanEncodeTable::from_data(&data).unwrap();
        assert_eq!(table.num_bits[0], 1);
        assert_eq!(table.num_bits[1], 1);
        let encoded = table.encode_single_stream(&data);

        let weights_raw = table.serialize_weights();
        let (parsed_weights, _) =
            crate::huffman::weights::parse_huffman_weights(&weights_raw).unwrap();
        let (decode_table, decode_log) =
            crate::huffman::weights::build_huffman_decode_table(&parsed_weights).unwrap();
        let decoded = crate::huffman::decode::decode_single_stream(
            &decode_table,
            decode_log,
            &encoded,
            data.len(),
        )
        .unwrap();
        assert_eq!(decoded, data);
    }
}

// RFC 8878 Zstandard Huffman tables with more than 128 explicit weights use
// two interleaved FSE states. The final Huffman weight remains implicit.
fn compress_weights(weights: &[u8]) -> Option<Vec<u8>> {
    use crate::bitstream::writer::BitWriter;
    use crate::fse::{
        encode::{FseEncodeState, FseEncodeTable},
        table_builder::{normalize_counts, serialize_fse_table_description},
    };
    let mut counts = [0u32; 13];
    for &w in weights {
        counts[w as usize] += 1;
    }
    if counts.iter().filter(|&&n| n > 0).count() == 1 {
        let dummy = if counts[0] == 0 { 0 } else { 1 };
        counts[dummy] = 1;
    }
    let last = counts.iter().rposition(|n| *n > 0).unwrap();
    let dist = normalize_counts(&counts[..=last], 6)
        .into_iter()
        .map(|n| n.abs())
        .collect::<Vec<_>>();
    let table = FseEncodeTable::from_distribution(&dist, 6);
    let mut out = serialize_fse_table_description(&dist, 6);
    let mut writer = BitWriter::new();
    let mut a = FseEncodeState::init(&table, weights[weights.len() - 1]);
    let mut b = FseEncodeState::init(&table, weights[weights.len() - 2]);
    for (i, &w) in weights[..weights.len() - 2].iter().rev().enumerate() {
        if i % 2 == 0 {
            a.encode_symbol(&mut writer, w);
        } else {
            b.encode_symbol(&mut writer, w);
        }
    }
    if weights.len() % 2 == 0 {
        a.flush(&mut writer, 6);
        b.flush(&mut writer, 6);
    } else {
        b.flush(&mut writer, 6);
        a.flush(&mut writer, 6);
    }
    writer.close_reverse_stream();
    out.extend(writer.into_bytes());
    if out.len() >= 128 {
        return None;
    }
    let mut result = Vec::with_capacity(out.len() + 1);
    result.push(out.len() as u8);
    result.extend(out);
    Some(result)
}
