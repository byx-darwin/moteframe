//! Typed spelling of the same joined lifecycle; physical stores and proofs are unchanged.
/// Two concrete stores sharing the established joined directory and lease semantics.
/// The historical `JoinedHistory<'a, D>` spelling retains its trait-object defaults.
pub type TypedJoinedHistory<'a, L, R, const D: usize> = crate::JoinedHistory<'a, D, L, R>;
