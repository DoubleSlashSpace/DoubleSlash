//! QR symbols for invite links.
//!
//! The symbol is the link the client already minted. Showing it does not
//! create a second invite. A link that does not fit in one symbol is refused
//! rather than truncated: a partial invite cannot be redeemed.

use qrcode::types::Color;
use qrcode::{EcLevel, QrCode};

/// Quiet zone a scanner expects around the symbol, in modules.
const QUIET: usize = 4;

/// Byte-mode capacity of the largest symbol at the lowest error correction.
const MAX_BYTES: usize = 2953;

/// Rows of a QR symbol for `data`, including the quiet zone.
///
/// Each row is `1` for a dark module and `0` for a light one. Every row has
/// the same length. `None` when `data` is empty or does not fit.
pub fn qr_rows(data: &str) -> Option<Vec<String>> {
    if data.is_empty() || data.len() > MAX_BYTES {
        return None;
    }
    let bytes = data.as_bytes();
    let code = QrCode::with_error_correction_level(bytes, EcLevel::M)
        .or_else(|_| QrCode::with_error_correction_level(bytes, EcLevel::L))
        .ok()?;
    let modules = code.width();
    let side = modules + QUIET * 2;
    let mut rows = Vec::with_capacity(side);
    for y in 0..side {
        let mut row = String::with_capacity(side);
        for x in 0..side {
            let inside = x >= QUIET && y >= QUIET && x < modules + QUIET && y < modules + QUIET;
            let dark = inside && code[(x - QUIET, y - QUIET)] == Color::Dark;
            row.push(if dark { '1' } else { '0' });
        }
        rows.push(row);
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_link_is_a_square_with_a_finder() {
        let rows = qr_rows("https://doubleslash.space/i#abc").expect("short link fits");
        let side = rows[0].len();
        assert!(side >= 21 + QUIET * 2);
        assert!(rows
            .iter()
            .all(|row| row.len() == side && row.chars().all(|c| c == '0' || c == '1')));
        // The top-left finder sits just inside the quiet zone: a dark run,
        // then a light run between its borders.
        let finder = &rows[QUIET][QUIET..QUIET + 7];
        assert_eq!(finder, "1111111");
        assert_eq!(&rows[QUIET + 1][QUIET..QUIET + 7], "1000001");
        // The quiet zone itself stays light.
        assert!(rows[0].chars().all(|c| c == '0'));
    }

    #[test]
    fn an_invite_sized_link_still_fits() {
        let payload = "https://doubleslash.space/i#".to_owned() + &"a".repeat(400);
        let rows = qr_rows(&payload).expect("a few hundred bytes fit");
        assert_eq!(rows.len(), rows[0].len());
    }

    #[test]
    fn empty_and_oversized_links_are_refused() {
        assert!(qr_rows("").is_none());
        assert!(qr_rows(&"a".repeat(MAX_BYTES + 1)).is_none());
    }
}
