use super::generated::mmdflux_sans_v1;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordedMetricsProfile {
    units_per_em: u16,
    advance_scale: f64,
    advances: &'static [(char, u16)],
}

impl RecordedMetricsProfile {
    pub(crate) fn mmdflux_sans_v1() -> Self {
        Self {
            units_per_em: mmdflux_sans_v1::UNITS_PER_EM,
            advance_scale: mmdflux_sans_v1::ADVANCE_SCALE,
            advances: mmdflux_sans_v1::ADVANCES,
        }
    }

    pub(crate) fn measure_scalar_width(&self, font_size: f64, ch: char) -> f64 {
        if ch == '\t' {
            return self.space_width(font_size) * 4.0;
        }
        if ch.is_control() {
            return 0.0;
        }
        if is_combining_mark(ch) {
            return 0.0;
        }
        if let Some(width) = self.recorded_width(font_size, ch) {
            return width;
        }
        if is_space_separator(ch) {
            return self.space_width(font_size);
        }
        if is_known_wide_scalar(ch) {
            return font_size;
        }
        font_size * 0.56
    }

    fn space_width(&self, font_size: f64) -> f64 {
        self.recorded_width(font_size, ' ')
            .unwrap_or(font_size * 0.25)
    }

    fn recorded_width(&self, font_size: f64, ch: char) -> Option<f64> {
        let index = self
            .advances
            .binary_search_by_key(&ch, |(codepoint, _)| *codepoint)
            .ok()?;
        let advance_units = self.advances[index].1;
        Some(advance_units as f64 / self.units_per_em as f64 * font_size * self.advance_scale)
    }
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
    )
}

fn is_space_separator(ch: char) -> bool {
    matches!(
        ch as u32,
        0x00A0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000
    )
}

fn is_known_wide_scalar(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2329..=0x232A
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
    )
}
