//! Lilienstern: a deeply configurable grand-strategy simulation engine.
//!
//! A game is data: a map of places (generated or imported from GeoJSON), factions, and rules in
//! two layers, both written in the same script files:
//!
//! * the **economy** in [GlassBox](glassbox): units in places taking and giving resources,
//!   agents carrying them along the map, values per place that spread and fade;
//! * the **politics** in Lilienstern's own blocks: events, decisions the factions' AI weighs
//!   and takes, and timed modifiers, with conditions and effects scoped to a faction and a
//!   place.
//!
//! Every number can be a parameter (`$name`), and experiments run a game under several
//! parameter sets and seeds. Runs are deterministic.

pub mod city;
pub mod experiment;
pub mod game;
pub mod geojson;
pub mod params;
pub mod rng;
pub mod script;
pub mod world;

pub use game::Game;
pub use params::Params;
pub use world::World;
