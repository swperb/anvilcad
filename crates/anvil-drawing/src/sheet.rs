//! Sheets: paper size, drawing scale and the projection method (see `docs/re/drawings.md`,
//! "Sheets").

use serde::{Deserialize, Serialize};
use std::fmt;

/// A standard paper size. Every sheet is landscape: width is the longer side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SheetSize {
    A4,
    A3,
    A2,
    A1,
    A0,
    AnsiA,
    AnsiB,
    AnsiC,
    AnsiD,
    AnsiE,
}

impl SheetSize {
    pub const ALL: [SheetSize; 10] = [
        SheetSize::A4,
        SheetSize::A3,
        SheetSize::A2,
        SheetSize::A1,
        SheetSize::A0,
        SheetSize::AnsiA,
        SheetSize::AnsiB,
        SheetSize::AnsiC,
        SheetSize::AnsiD,
        SheetSize::AnsiE,
    ];

    /// Width and height in millimetres, landscape. ISO 216 sizes are whole millimetres;
    /// ANSI/ASME Y14.1 sizes are whole inches.
    pub fn size_mm(self) -> (f64, f64) {
        const IN: f64 = 25.4;
        match self {
            SheetSize::A4 => (297.0, 210.0),
            SheetSize::A3 => (420.0, 297.0),
            SheetSize::A2 => (594.0, 420.0),
            SheetSize::A1 => (841.0, 594.0),
            SheetSize::A0 => (1189.0, 841.0),
            SheetSize::AnsiA => (11.0 * IN, 8.5 * IN),
            SheetSize::AnsiB => (17.0 * IN, 11.0 * IN),
            SheetSize::AnsiC => (22.0 * IN, 17.0 * IN),
            SheetSize::AnsiD => (34.0 * IN, 22.0 * IN),
            SheetSize::AnsiE => (44.0 * IN, 34.0 * IN),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SheetSize::A4 => "A4 (297 x 210 mm)",
            SheetSize::A3 => "A3 (420 x 297 mm)",
            SheetSize::A2 => "A2 (594 x 420 mm)",
            SheetSize::A1 => "A1 (841 x 594 mm)",
            SheetSize::A0 => "A0 (1189 x 841 mm)",
            SheetSize::AnsiA => "ANSI A (11 x 8.5 in)",
            SheetSize::AnsiB => "ANSI B (17 x 11 in)",
            SheetSize::AnsiC => "ANSI C (22 x 17 in)",
            SheetSize::AnsiD => "ANSI D (34 x 22 in)",
            SheetSize::AnsiE => "ANSI E (44 x 34 in)",
        }
    }

    /// The size's name alone, for the title block: "A3", "ANSI B".
    pub fn short_label(self) -> &'static str {
        match self {
            SheetSize::A4 => "A4",
            SheetSize::A3 => "A3",
            SheetSize::A2 => "A2",
            SheetSize::A1 => "A1",
            SheetSize::A0 => "A0",
            SheetSize::AnsiA => "ANSI A",
            SheetSize::AnsiB => "ANSI B",
            SheetSize::AnsiC => "ANSI C",
            SheetSize::AnsiD => "ANSI D",
            SheetSize::AnsiE => "ANSI E",
        }
    }

    pub fn is_iso(self) -> bool {
        matches!(self, SheetSize::A4 | SheetSize::A3 | SheetSize::A2 | SheetSize::A1 | SheetSize::A0)
    }

    /// The projection method that goes with the size's standard: first angle for ISO
    /// sizes, third angle for ANSI sizes.
    pub fn usual_projection(self) -> Projection {
        if self.is_iso() { Projection::FirstAngle } else { Projection::ThirdAngle }
    }
}

/// Where projected views sit around the front view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Projection {
    /// ISO (European) practice: each view is drawn on the far side of the front view
    /// from where it is seen, so the top view goes below and the right view to the left.
    FirstAngle,
    /// ASME (American) practice: each view is drawn on the side it is seen from, so the
    /// top view goes above and the right view to the right.
    ThirdAngle,
}

impl Projection {
    pub const ALL: [Projection; 2] = [Projection::FirstAngle, Projection::ThirdAngle];

    pub fn label(self) -> &'static str {
        match self {
            Projection::FirstAngle => "First angle",
            Projection::ThirdAngle => "Third angle",
        }
    }
}

/// A drawing scale, sheet length : model length, as `1:2` (half size) or `2:1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scale {
    pub sheet: u32,
    pub model: u32,
}

/// The scales ISO 5455 recommends, largest first.
pub const STANDARD_SCALES: [Scale; 15] = [
    Scale { sheet: 50, model: 1 },
    Scale { sheet: 20, model: 1 },
    Scale { sheet: 10, model: 1 },
    Scale { sheet: 5, model: 1 },
    Scale { sheet: 2, model: 1 },
    Scale { sheet: 1, model: 1 },
    Scale { sheet: 1, model: 2 },
    Scale { sheet: 1, model: 5 },
    Scale { sheet: 1, model: 10 },
    Scale { sheet: 1, model: 20 },
    Scale { sheet: 1, model: 50 },
    Scale { sheet: 1, model: 100 },
    Scale { sheet: 1, model: 200 },
    Scale { sheet: 1, model: 500 },
    Scale { sheet: 1, model: 1000 },
];

impl Scale {
    pub const FULL: Scale = Scale { sheet: 1, model: 1 };

    /// Sheet millimetres per model millimetre.
    pub fn factor(self) -> f64 {
        self.sheet.max(1) as f64 / self.model.max(1) as f64
    }

    /// Reads `1:2`, `2:1`, `1/2` or `1 : 2`. Both sides must be whole numbers from 1 to
    /// 10000.
    pub fn parse(text: &str) -> Result<Scale, String> {
        let t = text.trim();
        let (a, b) = t
            .split_once(':')
            .or_else(|| t.split_once('/'))
            .ok_or_else(|| "write the scale as two whole numbers, as 1:2 or 2:1".to_string())?;
        let num = |s: &str| -> Result<u32, String> {
            match s.trim().parse::<u32>() {
                Ok(v) if (1..=10_000).contains(&v) => Ok(v),
                _ => Err("each side of the scale must be a whole number from 1 to 10000".to_string()),
            }
        };
        Ok(Scale { sheet: num(a)?, model: num(b)? })
    }

    /// The largest standard scale at which something `size` model millimetres across
    /// fits in `room` sheet millimetres.
    pub fn fitting(size: f64, room: f64) -> Scale {
        if size.is_nan() || room.is_nan() || size <= 0.0 || room <= 0.0 {
            return Scale::FULL;
        }
        STANDARD_SCALES
            .iter()
            .copied()
            .find(|s| size * s.factor() <= room)
            .unwrap_or(Scale { sheet: 1, model: 1000 })
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.sheet, self.model)
    }
}

/// A sheet's settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub size: SheetSize,
    pub scale: Scale,
    pub projection: Projection,
}

impl Default for Sheet {
    fn default() -> Self {
        Sheet { size: SheetSize::A3, scale: Scale::FULL, projection: Projection::FirstAngle }
    }
}

/// The margin between the paper's edge and the drawing frame, in millimetres.
pub const FRAME_MARGIN: f64 = 10.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_landscape_and_standard() {
        assert_eq!(SheetSize::A4.size_mm(), (297.0, 210.0));
        assert_eq!(SheetSize::A0.size_mm(), (1189.0, 841.0));
        for (s, want) in [(SheetSize::AnsiA, (279.4, 215.9)), (SheetSize::AnsiE, (1117.6, 863.6))] {
            let (w, h) = s.size_mm();
            assert!((w - want.0).abs() < 1e-9 && (h - want.1).abs() < 1e-9, "{s:?}");
        }
        for s in SheetSize::ALL {
            let (w, h) = s.size_mm();
            assert!(w > h, "{s:?}");
        }
    }

    #[test]
    fn scales_parse_and_fit() {
        assert_eq!(Scale::parse("1:2"), Ok(Scale { sheet: 1, model: 2 }));
        assert_eq!(Scale::parse(" 2 / 1 "), Ok(Scale { sheet: 2, model: 1 }));
        assert!(Scale::parse("0:1").is_err());
        assert!(Scale::parse("1.5:1").is_err());
        assert!(Scale::parse("half").is_err());
        assert_eq!(Scale::parse("1:2").map(|s| s.factor()), Ok(0.5));
        assert_eq!(Scale::fitting(100.0, 150.0), Scale::FULL);
        assert_eq!(Scale::fitting(100.0, 99.0), Scale { sheet: 1, model: 2 });
        assert_eq!(Scale::fitting(10.0, 150.0), Scale { sheet: 10, model: 1 });
        assert_eq!(Scale::fitting(0.0, 150.0), Scale::FULL);
        assert_eq!(Scale { sheet: 1, model: 20 }.to_string(), "1:20");
    }
}
