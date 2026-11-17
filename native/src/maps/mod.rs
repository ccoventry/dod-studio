//! The game's map files: the BSP reader, a demo's map reference and whether
//! the map is installed, and downloading a missing one (#604).

pub mod bsp;
pub mod bsp_entities;
pub mod map_check;
pub mod map_fetch;
pub mod player_origins;

pub use map_check::{MapReference, MapStatus, check_demo, map_reference};

pub use map_fetch::{DEFAULT_MIRROR, FetchOutcome, fetch_map, map_url};
