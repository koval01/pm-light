//! Numbers between counts and drawing. A cast can lose a sign or digits, so each kind is done here,
//! once, within the range it is used in.

/// A pixel count as a drawing number. Pictures here stay far below 65 536 pixels a side, where
/// every whole number is exact in an `f32`.
pub fn float(count: u32) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

/// The same for a count of things, such as letters in an address.
pub fn float_of(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

/// A colour channel from a drawing number, held to a byte's range first.
pub const fn channel(value: f32) -> u8 {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=255 just before")]
    let byte = value.round().clamp(0.0, 255.0) as u8;
    byte
}

/// A length in pixels from a drawing number, held to what a picture here can be.
pub fn pixels(value: f32) -> u32 {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=65535 just before")]
    let whole = value.round().clamp(0.0, f32::from(u16::MAX)) as u32;
    whole
}

/// A size to show, to three digits: past 2^53 bytes the last ones do not matter.
#[expect(clippy::cast_precision_loss, reason = "a size is shown to three digits")]
pub const fn approximate(count: u64) -> f64 {
    count as f64
}

/// How much of `whole` is used, from nothing to all of it, for a bar.
pub fn share(used: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    #[expect(clippy::cast_possible_truncation, reason = "a fraction between 0 and 1, drawn as a bar")]
    let fraction = (approximate(used) / approximate(whole)).clamp(0.0, 1.0) as f32;
    fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_stays_between_nothing_and_all() {
        assert!(share(0, 0).abs() < f32::EPSILON);
        assert!((share(1, 4) - 0.25).abs() < 1e-6);
        assert!((share(9, 4) - 1.0).abs() < f32::EPSILON);
        assert!((share(u64::MAX, u64::MAX) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn out_of_range_values_are_held_at_the_edges() {
        assert_eq!(channel(-3.0), 0);
        assert_eq!(channel(300.0), 255);
        assert_eq!(channel(f32::NAN), 0);
        assert_eq!(pixels(-1.0), 0);
        assert_eq!(pixels(1e9), 65_535);
        assert_eq!(float(70_000), 65_535.0);
    }
}
