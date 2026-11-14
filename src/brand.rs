//! Авторство и защита подписи.
//!
//! Имя автора хранится в двух независимых видах (открытом и XOR-замаскированном) плюс его
//! FNV-1a хеш. Несовпадение любого из них ломает сборку (const-проверка) и, если бинарник
//! пропатчен после сборки, отражается в «О программе» и в логе.
//! Это защита от случайной и небрежной подмены подписи; от осознанного форка с переписанным
//! кодом никакая подпись в открытом исходнике не защитит — для этого есть лицензия (LICENSE).

pub const AUTHOR: &str = "p222222p222";

const MASK: u8 = 0x5A;
const AUTHOR_MASKED: [u8; 11] = [42, 104, 104, 104, 104, 104, 104, 42, 104, 104, 104];
const AUTHOR_HASH: u64 = 0xd4fd6d63459f908d;

const fn fnv1a(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u64).wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

const fn same(a: &[u8], m: &[u8]) -> bool {
    if a.len() != m.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] ^ MASK != m[i] {
            return false;
        }
        i += 1;
    }
    true
}

// Сборка не пройдёт, если подпись изменена.
const _: () = assert!(fnv1a(AUTHOR.as_bytes()) == AUTHOR_HASH, "watermark: AUTHOR изменён");
const _: () = assert!(same(AUTHOR.as_bytes(), &AUTHOR_MASKED), "watermark: копии AUTHOR не совпадают");

/// Проверка во время работы (ловит правку готового exe).
#[inline(never)]
pub fn verify() -> bool {
    let a = std::hint::black_box(AUTHOR.as_bytes());
    fnv1a(a) == AUTHOR_HASH && same(a, &AUTHOR_MASKED)
}

/// Подпись для показа: настоящая или пометка о подмене.
pub fn signature() -> String {
    if verify() {
        format!("Автор: {AUTHOR}")
    } else {
        format!("Подпись автора повреждена: это изменённая сборка. Оригинальный автор — {AUTHOR}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn watermark_intact() {
        assert!(super::verify());
        assert_eq!(super::AUTHOR, "p222222p222");
    }
}
