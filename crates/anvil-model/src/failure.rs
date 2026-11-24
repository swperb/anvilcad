//! Why a feature failed, in words a user can act on (issue #88).
//!
//! The rebuild carries each failure as a [`Cause`]: the kernel's own error type where it
//! has one, or a sentence the feature code already wrote for the user. [`explain`] turns
//! it into a [`FeatureError`] that names the feature, says what went wrong, and says what
//! to try. Technical detail (face numbers, kernel internals) goes in `details`, which the
//! app shows only when asked and which never replaces the plain message.

use crate::document::FeatureKind;
use anvil_brep::KernelError;
use anvil_kernel::blend::BlendError;
use anvil_kernel::build::BuildError;
use anvil_kernel::offset::OffsetError;
use anvil_kernel::shell::ShellError;
use serde::Serialize;

/// Why a feature could not be built, as the rebuild sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// A sentence the feature code wrote for the user.
    Plain(String),
    /// What went wrong and what to try, both written by the feature code, which knows
    /// better than a general suggestion.
    Told { cause: String, suggestion: String },
    /// Building a solid from a profile (extrude, revolve) failed.
    Build(BuildError),
    /// A fillet or chamfer failed.
    Blend(BlendError),
    /// A shell failed.
    Shell(ShellError),
    /// Offsetting curves failed.
    Offset(OffsetError),
    /// The exact kernel failed.
    Kernel(KernelError),
}

impl Cause {
    /// A cause with its own suggestion: "the sketch's outline is open", "Close it, or …".
    pub fn told(cause: impl Into<String>, suggestion: impl Into<String>) -> Cause {
        Cause::Told { cause: cause.into(), suggestion: suggestion.into() }
    }
}

impl From<String> for Cause {
    fn from(s: String) -> Cause {
        Cause::Plain(s)
    }
}

impl From<&str> for Cause {
    fn from(s: &str) -> Cause {
        Cause::Plain(s.to_string())
    }
}

impl From<BuildError> for Cause {
    fn from(e: BuildError) -> Cause {
        Cause::Build(e)
    }
}

impl From<BlendError> for Cause {
    fn from(e: BlendError) -> Cause {
        Cause::Blend(e)
    }
}

impl From<ShellError> for Cause {
    fn from(e: ShellError) -> Cause {
        Cause::Shell(e)
    }
}

impl From<OffsetError> for Cause {
    fn from(e: OffsetError) -> Cause {
        Cause::Offset(e)
    }
}

impl From<KernelError> for Cause {
    fn from(e: KernelError) -> Cause {
        Cause::Kernel(e)
    }
}

/// A length or angle for a message: up to three decimals, without trailing zeros.
pub fn mm(x: f64) -> String {
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// The feature's sketch has been deleted.
pub fn sketch_missing() -> Cause {
    Cause::told(
        "the sketch it uses has been deleted",
        "Edit the feature and pick another sketch, or undo the deletion.",
    )
}

/// The feature's sketch is in the tree but did not build, or is suppressed.
pub fn sketch_failed() -> Cause {
    Cause::told(
        "the sketch it uses failed or is suppressed",
        "Fix or unsuppress that sketch first; a failed one is marked in the tree.",
    )
}

/// A failed feature, explained.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FeatureError {
    /// The feature's name in the tree, such as "Fillet2".
    pub feature: String,
    /// What went wrong, as a sentence fragment: "the radius is too large for an edge".
    pub cause: String,
    /// What to try, as a full sentence.
    pub suggestion: String,
    /// Technical detail for the Details section and the log, when there is any.
    pub details: Option<String>,
}

impl FeatureError {
    /// The whole message: "Fillet2 could not be built: … Try …".
    pub fn message(&self) -> String {
        format!("{} could not be built: {}. {}", self.feature, self.cause, self.suggestion)
    }
}

impl std::fmt::Display for FeatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// What a feature does, for suggestions that depend on it.
#[derive(Clone, Copy, PartialEq)]
enum Does {
    Sketch,
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Fillet,
    Chamfer,
    Shell,
    Hole,
    Pattern,
    Reference,
    Other,
}

fn does(kind: &FeatureKind) -> Does {
    use FeatureKind::*;
    match kind {
        Sketch(_) => Does::Sketch,
        Extrude(_) | Rib(_) => Does::Extrude,
        Revolve(_) => Does::Revolve,
        Sweep(_) => Does::Sweep,
        Loft(_) => Does::Loft,
        Fillet(_) | Dome(_) => Does::Fillet,
        Chamfer(_) => Does::Chamfer,
        Shell(_) => Does::Shell,
        Hole(_) => Does::Hole,
        LinearPattern(_) | CircularPattern(_) | SketchPattern(_) | Mirror(_) => Does::Pattern,
        Plane(_) | Axis(_) | Point(_) | CoordSys(_) => Does::Reference,
        _ => Does::Other,
    }
}

/// Drops a trailing full stop, since the message adds its own.
fn fragment(s: &str) -> String {
    s.trim().trim_end_matches('.').to_string()
}

/// The fallback suggestion for a feature of this kind.
fn general(d: Does) -> &'static str {
    match d {
        Does::Sketch => "Edit the sketch: look for relations or dimensions shown in red and remove or change one.",
        Does::Extrude | Does::Revolve | Does::Sweep | Does::Loft => {
            "Check its sketch is a closed outline that does not cross itself, then try other end conditions or sizes."
        }
        Does::Fillet | Does::Chamfer => "Try a smaller size, or fewer edges at once.",
        Does::Shell => "Try a thinner wall, or remove different faces.",
        Does::Hole => "Check the hole's sketch points lie on the body, and try a smaller size.",
        Does::Pattern => "Try fewer instances or a different spacing, so each copy meets the body.",
        Does::Reference => "Edit it and pick its references again.",
        Does::Other => "Edit the feature and change its inputs, or suppress it to see the rest of the part.",
    }
}

/// Turns the rebuild's `cause` for feature `name` of `kind` into a message a user can act on.
pub fn explain(name: &str, kind: &FeatureKind, cause: Cause) -> FeatureError {
    let d = does(kind);
    let gen = general(d).to_string();
    let (cause, suggestion, details): (String, String, Option<String>) = match cause {
        Cause::Plain(s) => (fragment(&s), gen, None),
        Cause::Told { cause, suggestion } => (fragment(&cause), suggestion, None),
        Cause::Build(e) => {
            let how = match e {
                BuildError::EmptyProfile => {
                    "Close the sketch's outline so every end meets another, or pick a sketch with a closed outline."
                }
                BuildError::ZeroDepth => "Give it a depth other than 0.",
                BuildError::ZeroAngle => "Give it an angle other than 0.",
                BuildError::ProfileCrossesAxis => {
                    "Move the sketch's outline to one side of the axis; it may touch the axis but not cross it."
                }
                BuildError::DraftTooLarge => "Try a smaller draft angle or a shorter depth.",
            };
            (fragment(&e.to_string()), how.into(), None)
        }
        Cause::Blend(e) => {
            let size = if d == Does::Chamfer { "distance" } else { "radius" };
            match e {
                BlendError::EdgeNotFound(_) => (
                    "an edge it rounds no longer exists, because an earlier change removed or split it"
                        .into(),
                    "Edit the feature and pick its edges again.".into(),
                    Some(e.to_string()),
                ),
                BlendError::Unsupported(_) => (
                    fragment(&e.to_string()),
                    "Try fewer edges at once, or edges between flat faces.".into(),
                    None,
                ),
                BlendError::TooLarge(_) => {
                    (fragment(&e.to_string()), format!("Try a smaller {size}, or fewer edges at once."), None)
                }
                BlendError::Invalid(_) => (fragment(&e.to_string()), gen, None),
            }
        }
        Cause::Shell(e) => {
            let how = match e {
                ShellError::TooThick => "Try a thinner wall.",
                ShellError::FoldsOver => {
                    "Try a wall thinner than the smallest fillet radius, or shell the part before adding small fillets."
                }
                ShellError::Invalid(_) => general(Does::Shell),
            };
            (fragment(&e.to_string()), how.into(), None)
        }
        Cause::Offset(e) => {
            let how = match e {
                OffsetError::NotAChain => "Pick curves that join end to end.",
                OffsetError::Collapsed => "Try a smaller offset.",
            };
            (fragment(&e.to_string()), how.into(), None)
        }
        Cause::Kernel(e) => {
            let detail = Some(e.to_string());
            let (what, how): (&str, String) = match &e {
                KernelError::EmptyProfile => (
                    "the sketch has no closed outline",
                    "Close the sketch's outline so every end meets another.".into(),
                ),
                KernelError::NonFinite => (
                    "a size or position is not a number",
                    "Check its dimensions and the equations they use.".into(),
                ),
                KernelError::OpenLoop { .. } => (
                    "the sketch's outline has a gap",
                    "Edit the sketch and join the open ends; they are shown as points with one curve.".into(),
                ),
                KernelError::DegenerateSegment { .. } => (
                    "the sketch has a curve of zero length",
                    "Edit the sketch and delete the zero-length curve.".into(),
                ),
                KernelError::DegenerateLoop(_) => (
                    "a closed outline in the sketch encloses no area",
                    "Edit the sketch and remove or widen that outline.".into(),
                ),
                KernelError::SelfIntersection { .. } => (
                    "the sketch's outlines cross each other",
                    "Edit the sketch so no outline crosses another; trim where they meet.".into(),
                ),
                KernelError::InvalidFrame => (
                    "its sketch plane or direction is not valid",
                    "Edit it and pick the plane, face or direction again.".into(),
                ),
                KernelError::InvalidDistance => ("its depth is zero", "Give it a depth other than 0.".into()),
                KernelError::Unsupported(_) => ("it uses something the modeler cannot build yet", gen),
                KernelError::InvalidInput(_) => ("its inputs do not make a valid shape", gen),
                KernelError::Topology(_) | KernelError::Boolean(_) => {
                    ("the result would not be a valid solid", gen)
                }
                KernelError::Tessellation(_) => (
                    "its surfaces could not be turned into a mesh for display",
                    "Try slightly different sizes; if it keeps failing, report it with the part attached."
                        .into(),
                ),
            };
            (what.to_string(), how, detail)
        }
    };
    FeatureError { feature: name.to_string(), cause, suggestion, details }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;
    use anvil_kernel::{v2, EdgeRef, FaceId};

    /// Signs of a Rust `{:?}` string or a raw identifier leaking into a message.
    fn looks_debug(s: &str) -> bool {
        ["{", "}", "::", "Some(", "None", "\"", "<", ">", "Err(", "Ok("].iter().any(|t| s.contains(t))
    }

    fn check(e: &FeatureError, name: &str) {
        let m = e.message();
        assert!(m.starts_with(&format!("{name} could not be built: ")), "{m}");
        assert!(!looks_debug(&e.cause) && !looks_debug(&e.suggestion), "{m}");
        assert!(!e.suggestion.is_empty() && e.suggestion.ends_with('.'), "{m}");
        assert!(!e.cause.ends_with('.'), "the message adds its own full stop: {m}");
    }

    /// Every variant of every kernel error. The `match`es fail to compile when a variant
    /// is added, so a new error cannot reach users without a message being written for it.
    fn every_cause() -> Vec<Cause> {
        let edge = EdgeRef::new(FaceId { feature: 3, index: 1 }, FaceId { feature: 3, index: 2 });
        let build = [
            BuildError::EmptyProfile,
            BuildError::ZeroDepth,
            BuildError::ZeroAngle,
            BuildError::ProfileCrossesAxis,
            BuildError::DraftTooLarge,
        ];
        for b in &build {
            match b {
                BuildError::EmptyProfile
                | BuildError::ZeroDepth
                | BuildError::ZeroAngle
                | BuildError::ProfileCrossesAxis
                | BuildError::DraftTooLarge => {}
            }
        }
        let blend = [
            BlendError::EdgeNotFound(edge),
            BlendError::Unsupported("fillets on cones".into()),
            BlendError::TooLarge("the radius is larger than a face next to the edge".into()),
            BlendError::Invalid("the edges do not meet".into()),
        ];
        for b in &blend {
            match b {
                BlendError::EdgeNotFound(_)
                | BlendError::Unsupported(_)
                | BlendError::TooLarge(_)
                | BlendError::Invalid(_) => {}
            }
        }
        let shell =
            [ShellError::Invalid("no faces to remove".into()), ShellError::TooThick, ShellError::FoldsOver];
        for s in &shell {
            match s {
                ShellError::Invalid(_) | ShellError::TooThick | ShellError::FoldsOver => {}
            }
        }
        let offset = [OffsetError::NotAChain, OffsetError::Collapsed];
        for o in &offset {
            match o {
                OffsetError::NotAChain | OffsetError::Collapsed => {}
            }
        }
        let kernel = [
            KernelError::EmptyProfile,
            KernelError::NonFinite,
            KernelError::OpenLoop { contour: 0, segment: 2, gap: 0.5 },
            KernelError::DegenerateSegment { contour: 0, segment: 1 },
            KernelError::DegenerateLoop(1),
            KernelError::SelfIntersection { point: glam::DVec2::new(1.0, 2.0) },
            KernelError::InvalidFrame,
            KernelError::InvalidDistance,
            KernelError::Topology("edge (1, 2) used 3 times".into()),
            KernelError::InvalidInput("Some(bad)".into()),
            KernelError::Unsupported("torus {r: 0}".into()),
            KernelError::Boolean("the result is not a closed solid (edge EdgeKey(1) -> EdgeKey(2))".into()),
            KernelError::Tessellation("InsertionError::NAN".into()),
        ];
        for k in &kernel {
            match k {
                KernelError::EmptyProfile
                | KernelError::NonFinite
                | KernelError::OpenLoop { .. }
                | KernelError::DegenerateSegment { .. }
                | KernelError::DegenerateLoop(_)
                | KernelError::SelfIntersection { .. }
                | KernelError::InvalidFrame
                | KernelError::InvalidDistance
                | KernelError::Topology(_)
                | KernelError::InvalidInput(_)
                | KernelError::Unsupported(_)
                | KernelError::Boolean(_)
                | KernelError::Tessellation(_) => {}
            }
        }
        let mut all = vec![
            Cause::Plain("the sketch this feature uses is missing.".into()),
            Cause::told("the sketch's outline is open", "Close the outline, or turn on Thin feature."),
        ];
        all.extend(build.into_iter().map(Cause::Build));
        all.extend(blend.into_iter().map(Cause::Blend));
        all.extend(shell.into_iter().map(Cause::Shell));
        all.extend(offset.into_iter().map(Cause::Offset));
        all.extend(kernel.into_iter().map(Cause::Kernel));
        all
    }

    #[test]
    fn every_cause_names_the_feature_and_says_what_to_try() {
        let kind = FeatureKind::Fillet(FilletFeature {
            edges: Vec::new(),
            radius: Dim::Value(1.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        });
        for cause in every_cause() {
            let raw = format!("{cause:?}");
            let e = explain("Fillet2", &kind, cause);
            check(&e, "Fillet2");
            // Kernel internals are kept, but only under Details.
            if raw.contains("EdgeKey") || raw.contains("InsertionError") {
                assert!(e
                    .details
                    .as_deref()
                    .is_some_and(|d| d.contains("EdgeKey") || d.contains("InsertionError")));
            }
        }
    }

    /// A 40 x 30 x 20 block on the Top plane, and its extrude's id.
    fn block() -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 30.0));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(20.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        (doc, base)
    }

    fn failed(doc: &mut Document, id: u32) -> FeatureError {
        let st = doc.rebuild();
        let e = st.errors.get(&id).cloned().unwrap_or_else(|| panic!("feature {id} built: {:?}", st.errors));
        let name = doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
        check(&e, &name);
        e
    }

    #[test]
    fn a_fillet_on_an_edge_that_is_gone_says_pick_the_edges_again() {
        let (mut doc, base) = block();
        let side = |e: u32| FaceId::side(base, e);
        // Opposite sides of the block share no edge, as after an edit removes one.
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![EdgeRef::new(side(0), side(2))],
            radius: Dim::Value(2.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let e = failed(&mut doc, id);
        assert_eq!(
            e.cause,
            "an edge it rounds no longer exists, because an earlier change removed or split it"
        );
        assert_eq!(e.suggestion, "Edit the feature and pick its edges again.");
        // The face numbers are for whoever debugs it, not the message.
        assert!(e.details.as_deref().is_some_and(|d| d.contains('<')), "{e:?}");
    }

    #[test]
    fn an_extrude_of_an_open_sketch_says_close_the_outline() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        let (a, b) = (sf.sketch.add_point(v2(0.0, 0.0)), sf.sketch.add_point(v2(10.0, 0.0)));
        sf.sketch.add_line(a, b);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let id = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let e = failed(&mut doc, id);
        assert!(e.message().to_lowercase().contains("closed") || e.message().contains("outline"), "{e}");
    }

    /// Issue #172: a fillet too large for its faces used to be refused by the exact kernel
    /// and then built by the faceted one into a broken body (1198 mm³ from this 24000 mm³
    /// block) with no error. Now it fails, the block is untouched, and the part stays exact.
    #[test]
    fn a_fillet_too_large_fails_and_leaves_the_part_exact() {
        for (blend, what) in [(100.0, "fillet"), (35.0, "chamfer")] {
            let (mut doc, base) = block();
            let side = |e: u32| FaceId::side(base, e);
            let edge = vec![EdgeRef::new(side(0), side(1))];
            let kind = if what == "fillet" {
                FeatureKind::Fillet(FilletFeature {
                    edges: edge,
                    radius: Dim::Value(blend),
                    profile: FilletProfile::Circular,
                    tangent_propagation: false,
                })
            } else {
                FeatureKind::Chamfer(ChamferFeature {
                    edges: edge,
                    distance: Dim::Value(blend),
                    kind: ChamferKind::EqualDistance,
                    tangent_propagation: false,
                })
            };
            let id = doc.add_feature(kind);
            // A later fillet that fits is still built, and on the exact kernel.
            let later = doc.add_feature(FeatureKind::Fillet(FilletFeature {
                edges: vec![EdgeRef::new(side(2), side(3))],
                radius: Dim::Value(2.0),
                profile: FilletProfile::Circular,
                tangent_propagation: false,
            }));
            let e = failed(&mut doc, id);
            assert_eq!(e.cause, format!("a {what} of {blend} does not fit on the faces next to this edge"));
            let size = if what == "fillet" { "radius" } else { "distance" };
            assert_eq!(e.suggestion, format!("Try a smaller {size}, or fewer edges at once."));
            let st = doc.rebuild();
            assert_eq!(st.errors.len(), 1, "{:?}", st.errors);
            assert_eq!(st.faceted_from, None);
            assert!(st.exact_features.contains(&later));
            // The block less the later 2 mm round on its 20 mm upright edge.
            let want = 24000.0 - (4.0 - std::f64::consts::PI) * 20.0;
            let v = st.body.mass_properties().volume;
            // Within 0.1 mm³ (the mesh of the round), well under the 17.2 mm³ it removes.
            assert!((v - want).abs() < 0.1, "{what}: {v} vs {want}");
        }
    }

    #[test]
    fn a_shell_thicker_than_the_part_says_try_a_thinner_wall() {
        let (mut doc, base) = block();
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let id = doc
            .add_feature(FeatureKind::Shell(ShellFeature { remove: vec![top], thickness: Dim::Value(25.0) }));
        let e = failed(&mut doc, id);
        // Too thick for the part, not a curved region's fault: the block has none (#173).
        assert_eq!(e.cause, "the wall thickness is too large for this part");
        assert_eq!(e.suggestion, "Try a thinner wall.");
    }
}
