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
use anvil_kernel::{EdgeRef, FaceId};
use anvil_sketch::{PointId, ProfileError, Sketch};
use serde::Serialize;

/// Why a feature could not be built, as the rebuild sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// A sentence the feature code wrote for the user.
    Plain(String),
    /// What went wrong and what to try, both written by the feature code, which knows
    /// better than a general suggestion.
    Told { cause: String, suggestion: String },
    /// Like [`Cause::Told`], with technical detail for the Details section, such as the
    /// exact kernel's own reason for refusing a feature.
    Detailed { cause: String, suggestion: String, details: String },
    /// Like [`Cause::Told`], about a sketch whose curves are open: `ends` are the points of
    /// sketch feature `sketch` where they stop, which the app marks.
    Open { sketch: u32, ends: Vec<PointId>, cause: String, suggestion: String },
    /// The sketch's curves do not make closed outlines.
    Profile(ProfileError),
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

    /// A cause with its own suggestion and technical detail for the Details section.
    pub fn detailed(
        cause: impl Into<String>,
        suggestion: impl Into<String>,
        details: impl Into<String>,
    ) -> Cause {
        Cause::Detailed { cause: cause.into(), suggestion: suggestion.into(), details: details.into() }
    }
}

/// The plain cause alone, for places that report it without naming a feature, such as a
/// section view.
impl std::fmt::Display for Cause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&describe(Does::Other, self.clone()).cause)
    }
}

impl From<ProfileError> for Cause {
    fn from(e: ProfileError) -> Cause {
        Cause::Profile(e)
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

/// The sketch's curves are open: a cause naming the loose ends of `sketch`, the sketch of
/// feature `id`, with its own words.
pub fn open_sketch(
    id: u32,
    sketch: &Sketch,
    cause: impl Into<String>,
    suggestion: impl Into<String>,
) -> Cause {
    Cause::Open { sketch: id, ends: sketch.open_ends(), cause: cause.into(), suggestion: suggestion.into() }
}

/// A point of a sketch: a loose end the app marks when a feature fails on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SketchPoint {
    /// The sketch feature's id.
    pub sketch: u32,
    /// The point, in that sketch.
    pub point: PointId,
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
    /// Edges of the part before this feature that the failure is about, such as those a
    /// fillet is too large for. The app marks them while the feature is being edited.
    pub edges: Vec<EdgeRef>,
    /// Faces of the part before this feature that the failure is about, such as those a
    /// shell folds over.
    pub faces: Vec<FaceId>,
    /// Sketch points the failure is about: the loose ends of a sketch whose curves do not
    /// close, for an extrude, revolve or sweep that needs a closed outline.
    pub points: Vec<SketchPoint>,
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
    let Described { cause, suggestion, details, edges, faces, points } = describe(does(kind), cause);
    FeatureError { feature: name.to_string(), cause, suggestion, details, edges, faces, points }
}

/// A cause in words, before it is given a feature's name.
struct Described {
    cause: String,
    suggestion: String,
    details: Option<String>,
    edges: Vec<EdgeRef>,
    faces: Vec<FaceId>,
    points: Vec<SketchPoint>,
}

fn describe(d: Does, cause: Cause) -> Described {
    let gen = general(d).to_string();
    let (mut edges, mut faces, mut points) = (Vec::new(), Vec::new(), Vec::new());
    let (cause, suggestion, details): (String, String, Option<String>) = match cause {
        Cause::Plain(s) => (fragment(&s), gen, None),
        Cause::Told { cause, suggestion } => (fragment(&cause), suggestion, None),
        Cause::Detailed { cause, suggestion, details } => (fragment(&cause), suggestion, Some(details)),
        Cause::Open { sketch, ends, cause, suggestion } => {
            points = ends.into_iter().map(|point| SketchPoint { sketch, point }).collect();
            (fragment(&cause), suggestion, None)
        }
        Cause::Profile(e) => {
            let (what, how) = match e {
                ProfileError::Empty => (
                    "its sketch has no closed outline",
                    "Draw a closed outline in the sketch, or pick a sketch that has one.",
                ),
                ProfileError::OpenContour { .. } => (
                    "its sketch's outline is open",
                    "Close the outline so every end meets another, or turn on Thin feature.",
                ),
                ProfileError::Branching { .. } => (
                    "its sketch has a point where three or more curves meet, so the outline is ambiguous",
                    "Edit the sketch so each end meets exactly one other curve: trim the extra curves or make them construction geometry.",
                ),
                ProfileError::SelfIntersecting => (
                    "its sketch's outlines cross each other",
                    "Edit the sketch so no outline crosses another; trim where they meet.",
                ),
            };
            (what.into(), how.into(), Some(e.to_string()))
        }
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
                BlendError::TooLarge { edges: ref on, size: asked, limit } => {
                    let what = if d == Does::Chamfer { "chamfer" } else { "fillet" };
                    let next = if on.len() == 1 {
                        "the faces next to the edge".to_string()
                    } else {
                        format!("the faces next to {} of its edges", on.len())
                    };
                    let detail = Some(e.to_string());
                    edges = on.clone();
                    if limit < 0.0005 {
                        (
                            format!("the {what} {size} {} mm does not fit on {next}", mm(asked)),
                            format!("Try a smaller {size}, or leave out the edges that do not fit."),
                            detail,
                        )
                    } else {
                        let rest =
                            if on.len() == 1 { "" } else { ", or leave out the edges that do not fit" };
                        (
                            format!(
                                "the {what} {size} {} mm does not fit on {next}, which allow at most {} mm",
                                mm(asked),
                                mm(limit)
                            ),
                            format!("Try a {size} under {} mm{rest}.", mm(limit)),
                            detail,
                        )
                    }
                }
                BlendError::Invalid(_) => (fragment(&e.to_string()), gen, None),
            }
        }
        Cause::Shell(e) => {
            if let ShellError::FoldsOver { faces: on } = &e {
                faces = on.clone();
            }
            let how = match e {
                ShellError::TooThick => "Try a thinner wall.",
                ShellError::FoldsOver { .. } => {
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
                OffsetError::Elliptic => {
                    "Draw the offset curves with lines, arcs and circles instead of an ellipse."
                }
            };
            (fragment(&e.to_string()), how.into(), None)
        }
        // The exact kernel's own refusal, as from an operation that calls it directly. It
        // names the edge by the exact body's numbering, which is no use to a user, so
        // only the sizes are said.
        Cause::Kernel(KernelError::BlendTooLarge { edge, blend, size, limit }) => {
            let detail = Some(KernelError::BlendTooLarge { edge, blend, size, limit }.to_string());
            match limit {
                Some(limit) if limit >= 0.0005 => (
                    format!(
                        "the {blend} size {} mm does not fit on the faces next to an edge, which allow at most {} mm",
                        mm(size),
                        mm(limit)
                    ),
                    format!("Try a size under {} mm.", mm(limit)),
                    detail,
                ),
                _ => (
                    format!("the {blend} size {} mm does not fit on the faces next to an edge", mm(size)),
                    "Try a smaller size, or leave out the edges that do not fit.".into(),
                    detail,
                ),
            }
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
                KernelError::InvalidInput(_) | KernelError::BlendTooLarge { .. } => {
                    ("its inputs do not make a valid shape", gen)
                }
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
    Described { cause, suggestion, details, edges, faces, points }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;
    use anvil_kernel::v2;

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
            BlendError::TooLarge { edges: vec![edge], size: 6.0, limit: 4.0 },
            BlendError::TooLarge { edges: vec![edge, edge], size: 6.0, limit: 0.0 },
            BlendError::Invalid("the edges do not meet".into()),
        ];
        for b in &blend {
            match b {
                BlendError::EdgeNotFound(_)
                | BlendError::Unsupported(_)
                | BlendError::TooLarge { .. }
                | BlendError::Invalid(_) => {}
            }
        }
        let shell = [
            ShellError::Invalid("no faces to remove".into()),
            ShellError::TooThick,
            ShellError::FoldsOver { faces: vec![FaceId { feature: 3, index: 4 }] },
        ];
        for s in &shell {
            match s {
                ShellError::Invalid(_) | ShellError::TooThick | ShellError::FoldsOver { .. } => {}
            }
        }
        let offset = [OffsetError::NotAChain, OffsetError::Collapsed, OffsetError::Elliptic];
        for o in &offset {
            match o {
                OffsetError::NotAChain | OffsetError::Collapsed | OffsetError::Elliptic => {}
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
            KernelError::BlendTooLarge {
                edge: anvil_brep::EdgeId(4),
                blend: "chamfer",
                size: 12.0,
                limit: Some(7.5),
            },
            KernelError::BlendTooLarge {
                edge: anvil_brep::EdgeId(4),
                blend: "fillet",
                size: 12.0,
                limit: None,
            },
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
                | KernelError::Tessellation(_)
                | KernelError::BlendTooLarge { .. } => {}
            }
        }
        let profile = [
            ProfileError::Empty,
            ProfileError::OpenContour { point: 3 },
            ProfileError::Branching { point: 4 },
            ProfileError::SelfIntersecting,
        ];
        for p in &profile {
            match p {
                ProfileError::Empty
                | ProfileError::OpenContour { .. }
                | ProfileError::Branching { .. }
                | ProfileError::SelfIntersecting => {}
            }
        }
        let mut all = vec![
            Cause::Plain("the sketch this feature uses is missing.".into()),
            Cause::told("the sketch's outline is open", "Close the outline, or turn on Thin feature."),
            Cause::detailed(
                "the exact kernel could not build it",
                "Try a smaller radius.",
                "blend: EdgeKey(1) has no neighbour",
            ),
        ];
        all.push(Cause::Open {
            sketch: 1,
            ends: vec![3, 4],
            cause: "its sketch's outline is open.".into(),
            suggestion: "Close the outline.".into(),
        });
        all.extend(profile.into_iter().map(Cause::Profile));
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

    /// An open L of two lines, from (`x`, 10) down to (`x`, 0) and across to (`x` + 20, 0),
    /// on `plane`; its sketch's id, and the two loose ends' points.
    fn open_l(doc: &mut Document, plane: PlaneRef, x: f64) -> (u32, [PointId; 2]) {
        let mut sf = SketchFeature::new(plane);
        let a = sf.sketch.add_point(v2(x, 10.0));
        let corner = sf.sketch.add_point(v2(x, 0.0));
        let b = sf.sketch.add_point(v2(x + 20.0, 0.0));
        sf.sketch.add_line(a, corner);
        sf.sketch.add_line(corner, b);
        (doc.add_feature(FeatureKind::Sketch(sf)), [a, b])
    }

    /// An extrude, a thin extrude with capped ends, a revolve and a sweep of an open
    /// sketch each name its two loose ends, and only those, so the app can mark them
    /// (#199).
    #[test]
    fn features_on_an_open_sketch_name_its_loose_ends() {
        // Extrude.
        let mut doc = Document::new();
        let (sk, [a, b]) = open_l(&mut doc, PlaneRef::Top, 5.0);
        let id = doc.add_feature(extrude(sk, Dim::Value(10.0), BodyOp::Boss));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "its sketch's outline is open, so it encloses no area to extrude");
        assert_eq!(
            e.points,
            vec![SketchPoint { sketch: sk, point: a }, SketchPoint { sketch: sk, point: b }]
        );
        assert!(e.edges.is_empty() && e.faces.is_empty());

        // A thin wall with capped ends needs a closed outline too.
        let mut doc = Document::new();
        let (sk, [a, b]) = open_l(&mut doc, PlaneRef::Top, 5.0);
        let id = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            op: BodyOp::Boss,
            thin: Some(ThinWall {
                kind: ThinKind::OneDirection,
                thickness: Dim::Value(1.0),
                thickness2: Dim::Value(1.0),
                reverse: false,
                fillet: None,
                cap: Some(Dim::Value(1.0)),
            }),
            ..Default::default()
        }));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "Cap ends needs a closed outline, and the sketch's curves are open");
        assert_eq!(
            e.points,
            vec![SketchPoint { sketch: sk, point: a }, SketchPoint { sketch: sk, point: b }]
        );

        // A revolve about a centerline that one end of the L lies on: that end is closed
        // along the axis, so only the other is named. The axis's own ends are not.
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let a = sf.sketch.add_point(v2(5.0, 10.0));
        let corner = sf.sketch.add_point(v2(5.0, 0.0));
        let on_axis = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_line(a, corner);
        sf.sketch.add_line(corner, on_axis);
        let (p, q) = (sf.sketch.add_point(v2(0.0, -20.0)), sf.sketch.add_point(v2(0.0, 20.0)));
        let axis = sf.sketch.add_line(p, q);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let id =
            doc.add_feature(FeatureKind::Revolve(RevolveFeature { sketch: sk, axis, ..Default::default() }));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "its sketch's outline is open, and its ends are not both on the axis");
        assert_eq!(e.points, vec![SketchPoint { sketch: sk, point: a }]);

        // A sweep of the L, on the Right plane, along a line from the origin.
        let mut doc = Document::new();
        let (profile, [a, b]) = open_l(&mut doc, PlaneRef::Right, 5.0);
        let mut path = SketchFeature::new(PlaneRef::Top);
        let (p, q) = (path.sketch.add_point(v2(0.0, 0.0)), path.sketch.add_point(v2(50.0, 0.0)));
        path.sketch.add_line(p, q);
        let path = doc.add_feature(FeatureKind::Sketch(path));
        let id = doc.add_feature(FeatureKind::Sweep(crate::sweep::SweepFeature {
            profile: Some(profile),
            path,
            ..Default::default()
        }));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "its profile's outline is open");
        assert_eq!(
            e.points,
            vec![SketchPoint { sketch: profile, point: a }, SketchPoint { sketch: profile, point: b }]
        );
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
            // The upright edge is 20 mm tall between a 40 mm and a 30 mm wide side, so a
            // right-angle blend may set back at most 30 mm into the narrower one.
            let size = if what == "fillet" { "radius" } else { "distance" };
            assert_eq!(
                e.cause,
                format!(
                    "the {what} {size} {blend} mm does not fit on the faces next to the edge, which allow at most 30 mm"
                )
            );
            assert_eq!(e.suggestion, format!("Try a {size} under 30 mm."));
            assert_eq!(e.edges, vec![EdgeRef::new(side(0), side(1))]);
            assert!(e.faces.is_empty());
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

    /// Issue #200: a conic fillet too large for its faces used to be built by the exact
    /// kernel into a broken body (about 3611 mm³ from this 24000 mm³ block) with no error.
    /// Now it fails as a circular one does, with the same limit, and the block is
    /// untouched and still exact.
    #[test]
    fn a_conic_fillet_too_large_fails_with_the_limit() {
        let (mut doc, base) = block();
        let side = |e: u32| FaceId::side(base, e);
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![EdgeRef::new(side(0), side(1))],
            radius: Dim::Value(100.0),
            profile: FilletProfile::ConicRho { rho: Dim::Value(0.5) },
            tangent_propagation: false,
        }));
        let e = failed(&mut doc, id);
        assert_eq!(
            e.cause,
            "the fillet radius 100 mm does not fit on the faces next to the edge, which allow at most 30 mm"
        );
        assert_eq!(e.suggestion, "Try a radius under 30 mm.");
        assert_eq!(e.edges, vec![EdgeRef::new(side(0), side(1))]);
        let st = doc.rebuild();
        assert_eq!(st.errors.len(), 1, "{:?}", st.errors);
        assert_eq!(st.faceted_from, None);
        assert!(st.exact.is_some());
        assert_eq!(st.body.mass_properties().volume, 24000.0);
    }

    /// The other blends only the exact kernel builds are refused too when too large,
    /// rather than built into a broken body: the other round profiles with the same limit
    /// as a circular fillet, the offset and angle-distance chamfers (whose closed-form
    /// tools check the fit) with the exact kernel's reason.
    #[test]
    fn other_exact_only_blends_too_large_leave_the_block_untouched() {
        let profiles =
            [FilletProfile::ConicRadius { radius: Dim::Value(150.0) }, FilletProfile::CurvatureContinuous];
        let chamfers =
            [ChamferKind::OffsetFace, ChamferKind::AngleDistance { angle: Dim::Value(45.0), flip: false }];
        let kinds = profiles
            .into_iter()
            .map(|profile| {
                FeatureKind::Fillet(FilletFeature {
                    edges: vec![],
                    radius: Dim::Value(100.0),
                    profile,
                    tangent_propagation: false,
                })
            })
            .chain(chamfers.into_iter().map(|kind| {
                FeatureKind::Chamfer(ChamferFeature {
                    edges: vec![],
                    distance: Dim::Value(100.0),
                    kind,
                    tangent_propagation: false,
                })
            }));
        for mut kind in kinds {
            let (mut doc, base) = block();
            let edge = EdgeRef::new(FaceId::side(base, 0), FaceId::side(base, 1));
            let fillet = matches!(kind, FeatureKind::Fillet(_));
            match &mut kind {
                FeatureKind::Fillet(f) => f.edges = vec![edge],
                FeatureKind::Chamfer(c) => c.edges = vec![edge],
                _ => unreachable!(),
            }
            let id = doc.add_feature(kind.clone());
            let e = failed(&mut doc, id);
            if fillet {
                assert_eq!(
                    e.cause,
                    "the fillet radius 100 mm does not fit on the faces next to the edge, which allow at most 30 mm",
                    "{kind:?}"
                );
            }
            let st = doc.rebuild();
            assert_eq!(st.faceted_from, None, "{kind:?}");
            assert_eq!(st.body.mass_properties().volume, 24000.0, "{kind:?}");
        }
    }

    /// Chamfers only the exact kernel builds name the edge they do not fit and the largest
    /// distance that fits, from the exact kernel's typed refusal (#199). The upright edge
    /// of the 40 x 30 x 20 block lies between side 0, 40 mm wide, and side 1, 30 mm wide.
    /// A 30 degree chamfer sets back its distance along one face and distance x sin 30 /
    /// sin 120 = 0.577 of it along the other: along side 0 the 40 mm limits it, along side
    /// 1 the 30 mm (the other face would allow 30 / 0.577 = 52 and 40 / 0.577 = 69 mm). An
    /// offset-face chamfer across the right angle sets back its distance along both faces.
    #[test]
    fn exact_only_chamfers_too_large_name_the_edge_and_the_limit() {
        let angle = |flip| ChamferKind::AngleDistance { angle: Dim::Value(30.0), flip };
        for (kind, limit) in [(angle(false), 40), (angle(true), 30), (ChamferKind::OffsetFace, 30)] {
            let (mut doc, base) = block();
            let edge = EdgeRef::new(FaceId::side(base, 0), FaceId::side(base, 1));
            assert_eq!(edge.a, FaceId::side(base, 0), "the distance runs along side 0 unflipped");
            let id = doc.add_feature(FeatureKind::Chamfer(ChamferFeature {
                edges: vec![edge],
                distance: Dim::Value(100.0),
                kind: kind.clone(),
                tangent_propagation: false,
            }));
            let e = failed(&mut doc, id);
            assert_eq!(
                e.cause,
                format!(
                    "the chamfer distance 100 mm does not fit on the faces next to the edge, which allow at most {limit} mm"
                ),
                "{kind:?}"
            );
            assert_eq!(e.suggestion, format!("Try a distance under {limit} mm."), "{kind:?}");
            assert_eq!(e.edges, vec![edge], "{kind:?}");
            // Just under the limit it builds, exactly.
            if let Some(FeatureKind::Chamfer(c)) = doc.feature_mut(id).map(|f| &mut f.kind) {
                c.distance = Dim::Value(f64::from(limit) - 0.5);
            }
            let st = doc.rebuild();
            assert!(st.errors.is_empty(), "{kind:?}: {:?}", st.errors);
            assert!(st.exact_features.contains(&id), "{kind:?}");
        }
    }

    /// A conic fillet that fits still builds on the exact kernel. At rho 0.5 the round is
    /// a parabola, the quadratic Bezier from (r, 0) through the corner to (0, r) across a
    /// right angle, which leaves a third of the control triangle: r² / 6 of the corner's
    /// section is cut away along the 20 mm upright edge.
    #[test]
    fn a_conic_fillet_that_fits_builds_exactly() {
        let (mut doc, base) = block();
        let side = |e: u32| FaceId::side(base, e);
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![EdgeRef::new(side(0), side(1))],
            radius: Dim::Value(10.0),
            profile: FilletProfile::ConicRho { rho: Dim::Value(0.5) },
            tangent_propagation: false,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert_eq!(st.faceted_from, None);
        assert!(st.exact_features.contains(&id));
        let fine = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        let v: f64 = st
            .exact
            .as_ref()
            .expect("exact")
            .iter()
            .map(|b| {
                anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &fine).expect("tess")).volume
            })
            .sum();
        let want = 24000.0 - 10.0 * 10.0 / 6.0 * 20.0;
        assert!((v - want).abs() / want < 1e-6, "{v} vs {want}");
    }

    /// A fillet too large for some of its edges names those edges, and the tightest
    /// limit among them. On the 40 x 30 x 20 block a 25 mm round fits the 20 mm upright
    /// edge, between sides 40 and 30 mm wide, but not the top edges above 20 mm sides.
    #[test]
    fn a_fillet_too_large_for_some_edges_names_them_and_the_limit() {
        let (mut doc, base) = block();
        let side = |e: u32| FaceId::side(base, e);
        let top = FaceId { feature: base, index: FaceId::END_CAP };
        let (upright, top0, top1) =
            (EdgeRef::new(side(0), side(1)), EdgeRef::new(top, side(0)), EdgeRef::new(top, side(1)));
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![upright, top0, top1],
            radius: Dim::Value(25.0),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let e = failed(&mut doc, id);
        assert_eq!(
            e.cause,
            "the fillet radius 25 mm does not fit on the faces next to 2 of its edges, which allow at most 20 mm"
        );
        assert_eq!(e.suggestion, "Try a radius under 20 mm, or leave out the edges that do not fit.");
        assert_eq!(e.edges, vec![top0, top1]);
    }

    /// A shell that folds over a curved face names that face. (The exact kernel shells
    /// rounds properly, so this comes from the faceted kernel, on a meshed part.)
    #[test]
    fn a_shell_that_folds_over_names_the_faces() {
        let kind = FeatureKind::Shell(ShellFeature { remove: vec![], thickness: Dim::Value(3.0) });
        let rounds = vec![FaceId { feature: 5, index: 4 }, FaceId { feature: 5, index: 12 }];
        let e = explain("Shell1", &kind, Cause::Shell(ShellError::FoldsOver { faces: rounds.clone() }));
        check(&e, "Shell1");
        assert_eq!(
            e.cause,
            "the wall is thicker than the radius of a curved region (such as a small fillet)"
        );
        assert_eq!(
            e.suggestion,
            "Try a wall thinner than the smallest fillet radius, or shell the part before adding small fillets."
        );
        assert_eq!(e.faces, rounds);
        assert!(e.edges.is_empty());
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

    /// A sketch on the Top plane holding a `w` x `h` rectangle at the origin, and its id.
    fn rectangle_sketch(doc: &mut Document, plane: PlaneRef, w: f64, h: f64) -> u32 {
        let mut sf = SketchFeature::new(plane);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(w, h));
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn extrude(sketch: u32, depth: Dim, op: BodyOp) -> FeatureKind {
        FeatureKind::Extrude(ExtrudeFeature {
            sketch,
            end: EndCondition::Blind,
            depth,
            reverse: false,
            op,
            ..Default::default()
        })
    }

    /// Capped thin walls 6 mm thick at each end of a 10 mm extrusion would meet in the
    /// middle; the message gives both sizes and the largest cap that fits.
    #[test]
    fn caps_thicker_than_the_extrusion_give_both_sizes() {
        let mut doc = Document::new();
        let sk = rectangle_sketch(&mut doc, PlaneRef::Top, 40.0, 30.0);
        let id = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            op: BodyOp::Boss,
            thin: Some(ThinWall {
                kind: ThinKind::OneDirection,
                thickness: Dim::Value(2.0),
                thickness2: Dim::Value(1.0),
                reverse: false,
                fillet: None,
                cap: Some(Dim::Value(6.0)),
            }),
            ..Default::default()
        }));
        let e = failed(&mut doc, id);
        assert_eq!(e.feature, "Extrude-Thin1");
        assert_eq!(e.cause, "the caps are 6 mm thick at each end, and the extrusion is only 10 mm long");
        assert_eq!(e.suggestion, "Make the caps thinner than 5 mm, or the extrusion longer.");
    }

    /// A depth whose equation uses a variable that is not defined names the depth and
    /// the variable.
    #[test]
    fn a_depth_with_an_unknown_variable_names_both() {
        let mut doc = Document::new();
        let sk = rectangle_sketch(&mut doc, PlaneRef::Top, 40.0, 30.0);
        let id = doc.add_feature(extrude(sk, Dim::Expr("wall * 2".into()), BodyOp::Boss));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "the depth cannot be worked out: unknown variable 'wall'");
        assert_eq!(
            e.suggestion,
            "Edit the depth and fix its equation, or define the variables it uses in Equations."
        );
    }

    /// A fillet radius whose equation does not work out names the radius.
    #[test]
    fn a_fillet_radius_with_a_bad_equation_names_the_radius() {
        let (mut doc, base) = block();
        let side = |e: u32| FaceId::side(base, e);
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![EdgeRef::new(side(0), side(1))],
            radius: Dim::Expr("r_missing".into()),
            profile: FilletProfile::Circular,
            tangent_propagation: false,
        }));
        let e = failed(&mut doc, id);
        assert_eq!(e.feature, "Fillet1");
        assert_eq!(e.cause, "the radius cannot be worked out: unknown variable 'r_missing'");
    }

    /// A sketch on a reference plane that is suppressed names the plane.
    #[test]
    fn a_sketch_on_a_suppressed_plane_names_the_plane() {
        let mut doc = Document::new();
        let plane = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Top,
            offset: Dim::Value(10.0),
            ..Default::default()
        }));
        let sk = rectangle_sketch(&mut doc, PlaneRef::Feature(plane), 40.0, 30.0);
        let name = doc.feature(plane).map(|f| f.name.clone()).unwrap_or_default();
        if let Some(f) = doc.feature_mut(plane) {
            f.suppressed = true;
        }
        let e = failed(&mut doc, sk);
        assert_eq!(e.cause, format!("the plane {name} it is placed on failed or is suppressed"));
        assert_eq!(e.suggestion, format!("Fix or unsuppress {name}, or pick another plane."));
    }

    /// A sketch on a face that no longer exists names the feature that made the face.
    #[test]
    fn a_sketch_on_a_face_that_is_gone_names_the_feature_that_made_it() {
        let (mut doc, base) = block();
        let gone = FaceId { feature: base, index: 99 };
        let sk = rectangle_sketch(&mut doc, PlaneRef::Face(gone), 5.0, 5.0);
        let e = failed(&mut doc, sk);
        assert_eq!(
            e.cause,
            "the face it is placed on, made by Boss-Extrude1, no longer exists or is not flat"
        );
        assert!(e.suggestion.starts_with("Edit it and pick a flat face or a plane again"), "{e}");
    }

    /// A cut with nothing before it to cut says to move it after a boss.
    #[test]
    fn a_cut_with_no_body_says_move_it_after_one() {
        let mut doc = Document::new();
        let sk = rectangle_sketch(&mut doc, PlaneRef::Top, 40.0, 30.0);
        let id = doc.add_feature(extrude(sk, Dim::Value(10.0), BodyOp::Cut));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "there is no body to cut");
        assert_eq!(e.suggestion, "Move the cut after a feature that makes a body, or make it a boss.");
    }

    /// A sketch where three lines meet at one point cannot be read as an outline.
    #[test]
    fn an_extrude_of_a_branching_sketch_says_which_problem() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Top);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(40.0, 30.0));
        // A diagonal between opposite corners, so three curves meet at each and no end
        // is left open.
        let (a, b) = (sf.sketch.add_point(v2(0.0, 0.0)), sf.sketch.add_point(v2(40.0, 30.0)));
        sf.sketch.add_line(a, b);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let id = doc.add_feature(extrude(sk, Dim::Value(10.0), BodyOp::Boss));
        let e = failed(&mut doc, id);
        assert_eq!(
            e.cause,
            "its sketch has a point where three or more curves meet, so the outline is ambiguous"
        );
        assert!(e.details.is_some());
    }

    /// Flip side to cut on a revolve is refused with what to do instead.
    #[test]
    fn a_revolve_with_flip_side_says_turn_it_off() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(5.0, 0.0), v2(15.0, 10.0));
        let (a, b) = (sf.sketch.add_point(v2(0.0, 0.0)), sf.sketch.add_point(v2(0.0, 10.0)));
        let axis = sf.sketch.add_line(a, b);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let id = doc.add_feature(FeatureKind::Revolve(RevolveFeature {
            sketch: sk,
            axis,
            flip_side: true,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "Flip side to cut is not available for revolves yet");
        assert_eq!(e.suggestion, "Turn off Flip side to cut, and revolve the region to remove instead.");
    }

    /// An imported body whose triangles use a corner it does not have is reported as
    /// damaged, with the corner's number under Details.
    #[test]
    fn a_damaged_imported_body_says_import_it_again() {
        let mut doc = Document::new();
        let id = doc.add_feature(FeatureKind::ImportedBody(ImportedBodyFeature {
            source: "part.step".into(),
            replaces: Vec::new(),
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces: vec![ImportedFace { planar: true, triangles: vec![[0, 1, 7]] }],
            exact: None,
            adds: false,
        }));
        let e = failed(&mut doc, id);
        assert_eq!(e.cause, "the imported body is damaged: a triangle uses a corner the body does not have");
        assert_eq!(e.details.as_deref(), Some("vertex 7 of 3"));
    }

    /// A conic fillet the exact kernel refuses (here on an edge an earlier change
    /// removed) cannot fall back to the faceted kernel, which builds only circular ones.
    /// The message names the radius asked for and keeps the kernel's words under Details.
    #[test]
    fn a_conic_fillet_the_exact_kernel_refuses_names_the_radius() {
        let (mut doc, base) = block();
        let side = |e: u32| FaceId::side(base, e);
        let profile = FilletProfile::ConicRho { rho: Dim::Value(0.5) };
        let kind = profile.label().to_lowercase();
        let id = doc.add_feature(FeatureKind::Fillet(FilletFeature {
            edges: vec![EdgeRef::new(side(0), side(2))],
            radius: Dim::Value(2.5),
            profile,
            tangent_propagation: false,
        }));
        let e = failed(&mut doc, id);
        assert_eq!(
            e.cause,
            format!(
                "{kind} fillets are built only on the exact kernel, and it could not build this one with a radius of 2.5 mm"
            )
        );
        assert_eq!(
            e.suggestion,
            "Try a smaller radius or fewer edges, pick its edges again, or use a circular fillet, which the faceted kernel can also build."
        );
        assert_eq!(e.details.as_deref(), Some("an edge to blend no longer exists"));
    }
}
