//! The name an offline player gets, and the UUID every server gives that name.
//!
//! A launcher without a Microsoft account still starts a game: the offline name that plays, or
//! when none is chosen, one made up here and kept in `horizon-player.txt` beside `options.txt`,
//! in the same file and format Horizon's own game uses, so the same player keeps the same name
//! (and the same offline UUID) on every launch. The UUID is Java's
//! `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`: what a vanilla server computes for an
//! offline-mode player, so worlds and inventories find the same player every time.
use std::path::Path;

pub const FILE: &str = "horizon-player.txt";
const KEY: &str = "name:";

/// `Player` and five digits. Vanilla's client makes up `Player<millis % 1000>`; three digits
/// share a name too often for a game that keeps it.
pub fn generate(seed: u64) -> String {
    format!("Player{:05}", seed % 100_000)
}

/// The name kept in `dir`, created on first use. A file that can't be read or written still
/// yields a name for this run.
pub fn load_or_create(dir: &Path) -> String {
    let path = dir.join(FILE);
    if let Some(name) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t)) {
        return name;
    }
    let name = generate(seed());
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&path, format!("{KEY}{name}\n"));
    name
}

/// The offline UUID for `name`, as `UUID.nameUUIDFromBytes` makes it (`uuid` below).
pub fn uuid(name: &str) -> String {
    let digest = md5(&format!("OfflinePlayer:{name}"));
    let mut b = digest;
    b[6] = (b[6] & 0x0f) | 0x30; // version 3
    b[8] = (b[8] & 0x3f) | 0x80; // IETF variant
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn parse(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim_end_matches('\r').strip_prefix(KEY))
        .map(str::trim)
        .filter(|name| crate::account::valid_offline_name(name))
        .map(str::to_owned)
}

fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    // Two launchers making a name in the same millisecond still differ.
    let mut x = nanos ^ u64::from(std::process::id()).rotate_left(32);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^ (x >> 33)
}

/// MD5 (RFC 1321), the only hash an offline UUID needs.
fn md5(message: &str) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let bytes = message.as_bytes();
    let mut padded = bytes.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((bytes.len() as u64) * 8).to_le_bytes());
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in padded.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "horizon-launcher-offline-name-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn offline_uuids_are_the_ones_vanilla_computes() {
        // UUIDs a vanilla server gives these offline names (Java's nameUUIDFromBytes).
        assert_eq!(uuid("Alex"), "36532b5e-c442-3dbb-a24c-c7e55d0f979a");
        assert_eq!(uuid("Steve"), "5627dd98-e6be-3c21-b8a8-e92344183641");
    }

    #[test]
    fn generated_names_are_valid_and_the_file_is_kept() {
        let d = dir("kept");
        for seed in [0, 7, 99_999, 100_000, u64::MAX] {
            let name = generate(seed);
            assert!(crate::account::valid_offline_name(&name), "{name}");
            assert_eq!(name.len(), 11);
        }
        let first = load_or_create(&d);
        assert_eq!(
            std::fs::read_to_string(d.join(FILE)).unwrap(),
            format!("name:{first}\n")
        );
        assert_eq!(load_or_create(&d), first);
        std::fs::write(d.join(FILE), "name:Alex_2\r\n").unwrap();
        assert_eq!(load_or_create(&d), "Alex_2");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn invalid_names_are_replaced() {
        for text in ["", "name:", "name:has space", "name:été"] {
            let d = dir("invalid");
            std::fs::write(d.join(FILE), text).unwrap();
            let name = load_or_create(&d);
            assert!(crate::account::valid_offline_name(&name), "{text:?}");
            assert_eq!(load_or_create(&d), name);
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}
