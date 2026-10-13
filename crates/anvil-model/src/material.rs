//! Materials: physical properties for mass and stress, and the limits that
//! design-for-manufacturing checks need.
//!
//! Library values are nominal room-temperature figures from common datasheets (for
//! the usual temper or grade); a supplier's certificate governs a real part.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialClass {
    #[default]
    Metal,
    Plastic,
}

impl MaterialClass {
    pub fn label(self) -> &'static str {
        match self {
            MaterialClass::Metal => "Metal",
            MaterialClass::Plastic => "Plastic",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// g/cm^3
    pub density: f64,
    #[serde(default)]
    pub class: MaterialClass,
    /// Yield strength, MPa (0 when unknown).
    #[serde(default)]
    pub yield_strength: f64,
    /// Ultimate tensile strength, MPa.
    #[serde(default)]
    pub tensile_strength: f64,
    /// Young's modulus, GPa.
    #[serde(default)]
    pub elastic_modulus: f64,
    /// Thermal conductivity, W/(m K).
    #[serde(default)]
    pub thermal_conductivity: f64,
    /// Thinnest wall worth machining, mm.
    #[serde(default)]
    pub min_wall: f64,
    /// Smallest inside bend radius as a multiple of the sheet thickness, for sheet
    /// metal. `None` when the material is not bent as sheet.
    #[serde(default)]
    pub min_bend_ratio: Option<f64>,
}

/// (name, class, density g/cm3, yield MPa, UTS MPa, E GPa, k W/mK, min wall mm, bend ratio)
type Row = (&'static str, MaterialClass, f64, f64, f64, f64, f64, f64, Option<f64>);

impl Material {
    /// A material known only by name and density (as older documents stored them).
    /// Its other properties are unknown (zero); [`Material::resolved`] fills them in
    /// when the name is in the library.
    pub fn simple(name: &str, density: f64) -> Material {
        Material {
            name: name.into(),
            density,
            class: MaterialClass::Metal,
            yield_strength: 0.0,
            tensile_strength: 0.0,
            elastic_modulus: 0.0,
            thermal_conductivity: 0.0,
            min_wall: 0.0,
            min_bend_ratio: None,
        }
    }

    pub fn library() -> Vec<Material> {
        use MaterialClass::*;
        let rows: [Row; 26] = [
            ("Plain Carbon Steel", Metal, 7.80, 220.0, 400.0, 210.0, 50.0, 0.8, Some(1.0)),
            ("AISI 1018 Steel, cold drawn", Metal, 7.87, 370.0, 440.0, 205.0, 51.9, 0.8, Some(1.0)),
            ("ASTM A36 Steel", Metal, 7.85, 250.0, 400.0, 200.0, 50.0, 0.8, Some(1.0)),
            ("AISI 1045 Steel", Metal, 7.85, 310.0, 565.0, 206.0, 49.8, 0.8, Some(1.5)),
            ("AISI 4140 Steel, annealed", Metal, 7.85, 415.0, 655.0, 205.0, 42.6, 0.8, Some(2.0)),
            ("AISI 304", Metal, 8.00, 215.0, 505.0, 193.0, 16.2, 0.8, Some(1.0)),
            ("AISI 316 Stainless Steel", Metal, 8.00, 205.0, 515.0, 193.0, 16.3, 0.8, Some(1.0)),
            ("17-4 PH Stainless Steel, H1150", Metal, 7.80, 725.0, 930.0, 196.0, 18.3, 0.8, Some(3.0)),
            ("1060 Alloy", Metal, 2.70, 28.0, 69.0, 69.0, 234.0, 0.8, Some(0.5)),
            ("5052-H32 Aluminum", Metal, 2.68, 193.0, 228.0, 70.3, 138.0, 0.8, Some(1.0)),
            ("6061 Alloy", Metal, 2.70, 276.0, 310.0, 68.9, 167.0, 0.8, Some(2.5)),
            ("7075-T6 Aluminum", Metal, 2.81, 503.0, 572.0, 71.7, 130.0, 0.8, Some(4.0)),
            ("Brass", Metal, 8.50, 310.0, 385.0, 97.0, 115.0, 0.8, Some(1.0)),
            ("Copper", Metal, 8.90, 70.0, 220.0, 115.0, 388.0, 0.8, Some(0.5)),
            ("Titanium Ti-6Al-4V", Metal, 4.43, 880.0, 950.0, 113.8, 6.7, 0.8, Some(4.5)),
            ("Delrin (Acetal Homopolymer)", Plastic, 1.42, 72.0, 72.0, 3.1, 0.37, 1.5, None),
            ("UHMW Polyethylene", Plastic, 0.93, 21.0, 40.0, 0.69, 0.41, 2.0, None),
            ("HDPE", Plastic, 0.95, 26.0, 32.0, 1.1, 0.48, 2.0, None),
            ("PTFE (Teflon)", Plastic, 2.20, 9.0, 25.0, 0.5, 0.25, 2.0, None),
            ("Nylon 6/6", Plastic, 1.14, 70.0, 82.0, 2.8, 0.25, 1.5, None),
            ("PEEK", Plastic, 1.30, 97.0, 100.0, 3.6, 0.25, 1.5, None),
            ("Polycarbonate", Plastic, 1.20, 62.0, 65.0, 2.4, 0.20, 1.5, None),
            ("Acrylic (PMMA)", Plastic, 1.19, 65.0, 72.0, 3.2, 0.19, 1.5, None),
            ("ABS", Plastic, 1.04, 40.0, 44.0, 2.2, 0.17, 1.5, None),
            ("PLA", Plastic, 1.24, 60.0, 65.0, 3.5, 0.13, 1.5, None),
            ("PVC, rigid", Plastic, 1.40, 45.0, 52.0, 3.0, 0.19, 1.5, None),
        ];
        rows.into_iter()
            .map(|(name, class, density, ys, uts, e, k, wall, bend)| Material {
                name: name.into(),
                density,
                class,
                yield_strength: ys,
                tensile_strength: uts,
                elastic_modulus: e,
                thermal_conductivity: k,
                min_wall: wall,
                min_bend_ratio: bend,
            })
            .collect()
    }

    /// The library entry with this material's name, keeping this one's density (the
    /// user may have edited it). Documents saved before materials carried their full
    /// properties come back complete this way.
    pub fn resolved(&self) -> Material {
        match Material::library().into_iter().find(|m| m.name == self.name) {
            Some(m) if self.min_wall == 0.0 => Material { density: self.density, ..m },
            _ => self.clone(),
        }
    }

    /// Mass in grams of `volume` cubic millimetres.
    pub fn mass_of(&self, volume: f64) -> f64 {
        volume * self.density / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_complete_and_named_uniquely() {
        let lib = Material::library();
        let mut names: Vec<&str> = lib.iter().map(|m| m.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), lib.len());
        for m in &lib {
            assert!(
                m.density > 0.0 && m.tensile_strength >= m.yield_strength && m.min_wall > 0.0,
                "{}",
                m.name
            );
            assert_eq!(m.min_bend_ratio.is_some(), m.class == MaterialClass::Metal, "{}", m.name);
        }
        for want in
            ["ASTM A36 Steel", "AISI 316 Stainless Steel", "Delrin (Acetal Homopolymer)", "UHMW Polyethylene"]
        {
            assert!(lib.iter().any(|m| m.name == want), "{want}");
        }
    }

    #[test]
    fn old_documents_get_full_properties() {
        // Before this, a material was stored as just a name and a density.
        let old: Material = serde_json::from_str(r#"{"name":"6061 Alloy","density":2.71}"#).expect("parse");
        let m = old.resolved();
        assert_eq!(Material::simple("6061 Alloy", 2.7).resolved().min_bend_ratio, Some(2.5));
        assert_eq!(m.density, 2.71);
        assert_eq!(m.min_bend_ratio, Some(2.5));
        assert!((m.mass_of(1000.0) - 2.71).abs() < 1e-12);
    }
}
