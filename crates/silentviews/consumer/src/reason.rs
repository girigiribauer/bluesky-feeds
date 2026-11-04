#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Unmeasured,
    ThumbIllustration,
    ThumbFlatColors,
    ThumbFewNatureColors,
    Elongated,
    Illustration,
    Text,
    NearBlob,
    CenterSubject,
    Mirror,
    NoTexture,
    Object,
    PersonObject,
    Animal,
    ManmadeScene,
    GardenOrLookingDown,
    ThroughWindow,
    TextOcr,
    EdgeText,
    TooClose,
    TooCloseLines,
    ToonTile,
    PersonTile,
    ForegroundObject,
}

pub fn first_hit<const N: usize>(rules: [(Reason, bool); N]) -> Option<Reason> {
    rules.into_iter().find(|(_, hit)| *hit).map(|(r, _)| r)
}

pub fn any_nan(values: &[f64]) -> bool {
    values.iter().any(|v| v.is_nan())
}
