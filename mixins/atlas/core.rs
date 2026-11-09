// Horizon Atlas: a minimap, a world map and waypoints that share one map of
// the world. Every chunk the player loads is scanned into map columns (the
// map item's colours, heights and water depths) and kept on disk per world or
// server and per dimension, so the world map shows everywhere you have been.
// The minimap scrolls smoothly: its image is laid on a grid fixed to the
// world and shifted by the part of a pixel the player has moved. Waypoints
// show on both maps and in the world; the world map makes them where the
// pointer is. Both maps show the same cut of the world: the surface, or a
// cave or layer cross-section, each kept on disk on its own.
//
// This file is both the mod's Cargo library (its tests) and the module the
// bundle adds to the game; it uses only std, and the hooks hand it plain data.
//
// Built on Waypoints by Nirokx (MIT): the waypoint list, the form, the
// storage and the labels in the world. The map columns, their shading and the
// minimap's look follow Horizon's own minimap (`horizon-client-render`'s
// `minimap.rs`, MIT, Ludovic Marie and the Horizon contributors).
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

/// The mod's id (its data folder and its keys' lines), name and version.
pub const ID: &str = "atlas";
pub const NAME: &str = "Horizon Atlas";
pub const VERSION: &str = "0.2.0";
pub const DESCRIPTION: &str = "A smooth minimap, a world map of everywhere you have been and waypoints on both. M opens the map, B makes a waypoint, U lists them.";
/// The file the settings are kept in, in the mod's folder.
pub const SETTINGS_FILE: &str = "settings.txt";
/// The keys' names and defaults (`key.atlas.map` in `horizon-mods.txt`).
pub const KEY_SETTINGS: &str = "settings";
pub const DEFAULT_SETTINGS: &str = "key.keyboard.y";
pub const KEY_ENLARGE: &str = "enlarge";
pub const DEFAULT_ENLARGE: &str = "key.keyboard.z";
pub const KEY_ZOOM_IN: &str = "zoom_in";
pub const DEFAULT_ZOOM_IN: &str = "key.keyboard.right.bracket";
pub const KEY_ZOOM_OUT: &str = "zoom_out";
pub const DEFAULT_ZOOM_OUT: &str = "key.keyboard.left.bracket";
pub const KEY_TEMP: &str = "temporary";
pub const DEFAULT_TEMP: &str = "key.keyboard.h";
pub const KEY_MAP: &str = "map";
pub const KEY_CREATE: &str = "create";
pub const KEY_LIST: &str = "list";
pub const DEFAULT_MAP: &str = "key.keyboard.m";
pub const DEFAULT_CREATE: &str = "key.keyboard.b";
pub const DEFAULT_LIST: &str = "key.keyboard.u";

pub const OVERWORLD: &str = "minecraft:overworld";
pub const NETHER: &str = "minecraft:the_nether";
pub const END: &str = "minecraft:the_end";

/// The colours a waypoint can have: Minecraft's text colours and a brown.
pub const PALETTE: [u32; 16] = [
    0xFF5555, 0xFFAA00, 0xFFFF55, 0x55FF55, 0x00AA00, 0x55FFFF, 0x00AAAA, 0x5555FF, 0x0000AA,
    0xAA00AA, 0xFF55FF, 0xFFFFFF, 0xAAAAAA, 0x555555, 0xAA5500, 0x000000,
];

const NAME_MAX: usize = 32;
const COORD_MAX: usize = 9;
const LINE: f32 = 10.0;
const ROW: f32 = 24.0;

const INK: u32 = 0xFFFFFF;
const MUTED: u32 = 0xAAAAAA;
const ERROR: u32 = 0xFF5555;
const SHADE: u32 = 0xA000_0000;
const PANEL: u32 = 0xE010_1018;
const PANEL_EDGE: u32 = 0xFF50_5064;
const FIELD: u32 = 0xFF00_0000;
const FIELD_EDGE: u32 = 0xFF70_7070;
const FIELD_FOCUS: u32 = 0xFFFF_FFFF;
const ROW_BG: u32 = 0x3000_0000;
const LABEL_BG: u32 = 0x9000_0000;
/// The world map where nothing has been seen, and its bars.
const MAP_VOID: u32 = 0xFF00_0000;

/// What sort of waypoint one is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WpKind {
    #[default]
    Normal,
    /// Where the player died.
    Death,
    /// Gone when the game closes.
    Temporary,
}

/// The set a waypoint is in unless told otherwise.
pub const DEFAULT_SET: &str = "Default";

/// One waypoint. `pos` is a block position; `color` is `0xRRGGBB`.
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub pos: [i32; 3],
    pub dimension: String,
    pub color: u32,
    pub hidden: bool,
    pub set: String,
    pub kind: WpKind,
}

impl Default for Waypoint {
    fn default() -> Self {
        Self {
            name: String::new(),
            pos: [0; 3],
            dimension: OVERWORLD.to_owned(),
            color: 0xFFFFFF,
            hidden: false,
            set: DEFAULT_SET.to_owned(),
            kind: WpKind::Normal,
        }
    }
}

/// Where the player is, as the hooks read it: feet position, dimension and
/// yaw (0 faces south, 90 west).
#[derive(Clone, Debug, PartialEq)]
pub struct Here {
    pub pos: [f64; 3],
    pub dimension: String,
    pub yaw: f32,
}

/// The camera the last frame drew the world from: its position, the view's
/// yaw and pitch (a third person view facing the player already turned
/// round) and the vertical field of view in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

/// What the hooks know for the lines under the minimap: the biome the
/// player is in (`minecraft:plains`) and the world's time of day in ticks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MiniInfo {
    pub biome: Option<String>,
    pub day_time: Option<i64>,
    /// Block and sky light where the player stands.
    pub light: Option<(u8, u8)>,
    /// Rain and thunder, 0 to 1.
    pub weather: Option<(f32, f32)>,
}

/// What a dot on the minimap stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DotKind {
    Player,
    #[default]
    Mob,
    Item,
    Other,
}

impl DotKind {
    /// White players, yellow mobs, red items, purple the rest.
    pub fn color(self) -> u32 {
        match self {
            DotKind::Player => 0xFFFF_FFFF,
            DotKind::Mob => 0xFFFF_F050,
            DotKind::Item => 0xFFFF_2020,
            DotKind::Other => 0xFFB8_60FF,
        }
    }
    fn layer(self) -> u8 {
        match self {
            DotKind::Item => 0,
            DotKind::Other => 1,
            DotKind::Mob => 2,
            DotKind::Player => 3,
        }
    }
}

/// What an entity shows as when icons are on.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Icon {
    #[default]
    None,
    /// An item's icon (`minecraft:apple`).
    Item(String),
    /// A player's face: their skin sprite.
    Skin(String),
    /// A mob's face in the face sheet (`minecraft:zombie`).
    Mob(String),
}

/// An entity near the player, as a dot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dot {
    pub pos: [f64; 3],
    pub kind: DotKind,
    pub icon: Icon,
    pub name: Option<String>,
}

/// What a hook draws, in GUI pixels: a filled `[x, y, w, h]` in ARGB, a
/// line of text at its top left in RGB with or without a shadow, part `src`
/// (`[x, y, w, h]` in texels) of the image in map texture slot `slot`
/// stretched into `dst` and tinted ARGB, or a four-cornered shape in ARGB.
#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    Fill([f32; 4], u32),
    Text(String, [f32; 2], u32, bool),
    Image(usize, [f32; 4], [f32; 4], u32),
    /// The same of the mod's image (Horizon's `native_mod_image`).
    ModImage([f32; 4], [f32; 4], u32),
    Quad([[f32; 2]; 4], u32),
    /// An item's icon with its top left at the point, the size square.
    Item(String, [f32; 2], f32),
    /// A player's face from their skin sprite, into `[x, y, w, h]`.
    Skin(String, [f32; 4]),
    /// A vanilla GUI sprite (`gui/sprites/widget/button`) nine-sliced into a
    /// rectangle, tinted ARGB.
    Nine(&'static str, [f32; 4], u32),
    /// Text at its top left, ARGB (it can be see-through), scaled by the
    /// factor (1 is the GUI's font size), with or without a shadow.
    Label(String, [f32; 2], u32, f32, bool),
}

/// An arrow at `at`, `len` pixels from tip to tail, pointing `degrees`
/// clockwise from up: an `argb` head over a dark outline.
pub fn arrow(draws: &mut Vec<Draw>, at: [f32; 2], degrees: f32, len: f32, argb: u32) {
    let (s, c) = degrees.to_radians().sin_cos();
    let turn = |x: f32, y: f32| [at[0] + x * c - y * s, at[1] + x * s + y * c];
    let shape = |k: f32| {
        let (h, w) = (len * 0.5 * k, len * 0.4 * k);
        [turn(0.0, -h), turn(w, h), turn(0.0, h * 0.4), turn(-w, h)]
    };
    draws.push(Draw::Quad(shape(1.35), 0xFF10_1010));
    draws.push(Draw::Quad(shape(1.0), argb));
}

/// How near in height an entity `dy` blocks above (or below) the player
/// is: 1 up to `from` blocks away, falling to 0 over 32 blocks more.
pub fn height_fade(dy: f64, from: f32) -> f32 {
    (1.0 - (dy.abs() as f32 - from) / 32.0).clamp(0.0, 1.0)
}

/// `argb` with its colour times `light` and its alpha faded to 30% at
/// `near` 0.
pub fn fade(argb: u32, light: f32, near: f32) -> u32 {
    let [a, r, g, b] = argb.to_be_bytes();
    let alpha = (f32::from(a) * (0.3 + 0.7 * near)).round() as u32;
    let ch = |c: u8| (f32::from(c) * light).round().clamp(0.0, 255.0) as u32;
    alpha << 24 | ch(r) << 16 | ch(g) << 8 | ch(b)
}

/// A diamond `r` from the middle to each point.
pub fn diamond(draws: &mut Vec<Draw>, at: [f32; 2], r: f32, argb: u32) {
    let [x, y] = at;
    draws.push(Draw::Quad([[x, y - r], [x + r, y], [x, y + r], [x - r, y]], argb));
}

/// A round dot of radius `r` at `at` (an octagon: three quads).
pub fn disc(draws: &mut Vec<Draw>, at: [f32; 2], r: f32, argb: u32) {
    let a = r * 0.414;
    let p = |x: f32, y: f32| [at[0] + x, at[1] + y];
    draws.push(Draw::Quad([p(-a, -r), p(a, -r), p(r, -a), p(-r, -a)], argb));
    draws.push(Draw::Quad([p(-r, -a), p(r, -a), p(r, a), p(-r, a)], argb));
    draws.push(Draw::Quad([p(-r, a), p(r, a), p(a, r), p(-a, r)], argb));
}

/// Text in `rgb` with a black outline, as Xaero's compass letters.
pub fn outlined(draws: &mut Vec<Draw>, text: &str, at: [f32; 2], rgb: u32, scale: f32) {
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        let p = [at[0] + dx * scale, at[1] + dy * scale];
        draws.push(Draw::Label(text.into(), p, 0xFF10_1010, scale, false));
    }
    draws.push(Draw::Label(text.into(), at, 0xFF00_0000 | rgb, scale, false));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Overworld,
    Nether,
    End,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Overworld, Filter::Nether, Filter::End];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Overworld => "Overworld",
            Filter::Nether => "Nether",
            Filter::End => "End",
        }
    }

    pub fn matches(self, dimension: &str) -> bool {
        match self {
            Filter::All => true,
            Filter::Overworld => dimension == OVERWORLD,
            Filter::Nether => dimension == NETHER,
            Filter::End => dimension == END,
        }
    }
}

/// The editor's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    X,
    Y,
    Z,
}

impl Field {
    const ORDER: [Field; 4] = [Field::Name, Field::X, Field::Y, Field::Z];

    fn step(self, back: bool) -> Field {
        let at = Field::ORDER.iter().position(|&f| f == self).unwrap_or(0);
        let n = Field::ORDER.len();
        Field::ORDER[if back { (at + n - 1) % n } else { (at + 1) % n }]
    }
}

/// Where Cancel and Save on the form go: the game, the list or the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Game,
    List,
    Map,
}

/// The create / edit form. `index` is the waypoint being edited, None for a
/// new one.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    pub index: Option<usize>,
    pub name: String,
    pub coords: [String; 3],
    pub dimension: String,
    pub color: u32,
    pub hidden: bool,
    pub focus: Field,
    pub back: Back,
    pub error: Option<String>,
    /// The name is the suggested one, untouched: typing replaces it.
    pub suggested: bool,
}

impl Editor {
    fn field(&mut self, field: Field) -> &mut String {
        match field {
            Field::Name => &mut self.name,
            Field::X => &mut self.coords[0],
            Field::Y => &mut self.coords[1],
            Field::Z => &mut self.coords[2],
        }
    }
}

/// Where the settings screen is.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsView {
    pub page: Page,
    /// Which of the page's screens of buttons shows.
    pub sub: usize,
    pub search: String,
    pub searching: bool,
}

/// A button of the settings screen.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Opt(usize),
    Link(Page),
    List,
}

/// The settings on `view`'s screen: what the search finds, else its page.
fn settings_items(view: &SettingsView) -> Vec<Item> {
    let wanted = view.search.trim().to_lowercase();
    if !wanted.is_empty() {
        return (0..OPTS.len())
            .filter(|&i| OPTS[i].label.to_lowercase().contains(&wanted))
            .map(Item::Opt)
            .collect();
    }
    let mut items: Vec<Item> = (0..OPTS.len())
        .filter(|&i| OPTS[i].page == view.page)
        .map(Item::Opt)
        .collect();
    if view.page == Page::Main {
        items.extend(
            [Page::View, Page::BlockMap, Page::Radar, Page::Overlay, Page::Info, Page::Waypoints]
                .into_iter()
                .map(Item::Link),
        );
        items.push(Item::Link(Page::Misc));
        items.push(Item::List);
    }
    items
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Closed,
    List,
    Edit(Editor),
    Map,
    Settings(SettingsView),
}

/// What a click on the screen does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Toggle(usize),
    Delete(usize),
    Focus(Field),
    Color(u32),
    CycleDimension,
    ToggleHidden,
    Here,
    Save,
    Cancel,
    Close,
    /// The list, from the map.
    OpenList,
    /// The map follows the player again.
    Centre,
    /// A waypoint's marker on the world map: edit it.
    MapWaypoint(usize),
    /// A settings button: the next value of option `i`.
    Opt(usize),
    /// A settings slider and its rectangle: its value at the pointer.
    Slide(usize, [f32; 4]),
    GoPage(Page),
    /// Next (1) or previous (-1) screen of the page.
    SubPage(i32),
    SettingsBack,
    FocusSearch,
    /// The next dimension filter of the list.
    CycleFilter,
    /// The next waypoint set.
    CycleSet,
    /// A new set, named after the filter text.
    NewSet,
    /// Delete every waypoint of the set (click twice).
    ClearSet,
    FocusFilter,
    /// Edit the picked waypoint, else a new one.
    AddEdit,
    /// Say a waypoint in chat.
    Share(usize),
    /// A row of the list: pick that waypoint.
    Select(usize),
    /// Teleport to a waypoint (operators).
    Teleport(usize),
    /// The next map mode (Auto, Cave, Layer).
    CycleMapMode,
    /// The world map shows the next dimension's map.
    CycleMapDimension,
    /// Every waypoint of the set off, or all on again when none is on.
    ToggleAll,
    /// The Layer mode's height up or down a block (16 with Shift).
    LayerStep(i32),
}

/// A clickable rectangle `[x, y, w, h]` of the screen drawn this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub rect: [f32; 4],
    pub action: Action,
}

// ---- Map columns -------------------------------------------------------------

/// A column's colour is `kind | 0xRRGGBB`: what it is in the top byte, the
/// colour the player would see from above (the block's top texture, tinted
/// by its biome) in the rest. A kind of 0 is a column with nothing in it.
pub const LAND: u32 = 1 << 24;
pub const WATER: u32 = 2 << 24;
/// Tree tops: land, with their own edges.
pub const LEAF: u32 = 3 << 24;
/// Marks a column the map cuts through (solid at the cut's height).
pub const WALL: u32 = 0x80 << 24;
pub const KIND: u32 = 0x7F << 24;
pub const RGB: u32 = 0xFF_FFFF;
/// Columns that have been seen but have no colour show this, unseen ones
/// nothing.
const UNKNOWN: [u8; 4] = [20, 20, 20, 200];

/// What the maps show: the top of every column, or the floor under a
/// height (caves, the Nether, under a roof). Both maps always show the same
/// one, and each is kept in its own folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cut {
    /// The top block, as map items show it.
    Surface,
    /// A cross-section: from this height down. A column that is solid for
    /// `CUT_DEPTH` blocks from it is a wall (`| WALL`); else the first floor
    /// under the air below it shows.
    Below(i32),
}

/// How deep a cut looks for air before the column counts as a wall.
pub const CUT_DEPTH: i32 = 16;

impl Cut {
    /// Its folder in a dimension's map folder.
    pub fn folder(self) -> String {
        match self {
            Cut::Surface => "surface".into(),
            Cut::Below(y) => format!("below{y}"),
        }
    }
    pub fn label(self) -> String {
        match self {
            Cut::Surface => "Surface".into(),
            Cut::Below(y) => format!("Below Y {y}"),
        }
    }
}

/// How the maps choose their cut (Options > Mods > Atlas, `/atlas mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapMode {
    /// The surface in the open; the cave around the player under a roof,
    /// underground and in the Nether.
    Auto,
    /// Always the cave around the player.
    Cave,
    /// Always the cut at a height the player chose.
    Layer,
}

impl MapMode {
    pub const ALL: [MapMode; 3] = [MapMode::Auto, MapMode::Cave, MapMode::Layer];
    pub fn label(self) -> &'static str {
        match self {
            MapMode::Auto => "Auto",
            MapMode::Cave => "Cave",
            MapMode::Layer => "Layer",
        }
    }
}

/// The top of the 16-block band a height is in: caves are cut there, so
/// walking up and down a few blocks keeps the same map.
pub fn band_top(y: i32) -> i32 {
    y.div_euclid(CUT_DEPTH) * CUT_DEPTH + CUT_DEPTH - 1
}

/// The cut `mode` shows for a player whose head is at `head`: `covered`
/// when no sky light reaches it, `ceiling` in a dimension with a roof (the
/// Nether).
pub fn cut_for(mode: MapMode, layer_y: i32, head: i32, covered: bool, ceiling: bool) -> Cut {
    match mode {
        MapMode::Auto if !covered && !ceiling => Cut::Surface,
        MapMode::Auto | MapMode::Cave => Cut::Below(band_top(head)),
        MapMode::Layer => Cut::Below(layer_y),
    }
}

/// One chunk's map columns (`z * 16 + x`): the colour (`LAND`, `WATER`,
/// `| WALL`, with RGB), the height of the block that gave it, the depth of
/// the water over it, and the block light over it (0 to 15).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub color: [u32; 256],
    pub height: [i16; 256],
    pub depth: [u8; 256],
    pub light: [u8; 256],
}

/// A column: colour, height, water depth, block light.
pub type Column = (u32, i16, u8, u8);

impl Tile {
    pub fn blank(min_y: i16) -> Self {
        Self {
            color: [0; 256],
            height: [min_y; 256],
            depth: [0; 256],
            light: [0; 256],
        }
    }
    #[inline]
    pub fn column(&self, x: i32, z: i32) -> Column {
        let i = ((z & 15) * 16 + (x & 15)) as usize;
        (self.color[i], self.height[i], self.depth[i], self.light[i])
    }
}

/// How columns are shaded (Block Map Settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    /// 0 flat, 1 normal, 2 strong.
    pub slopes: usize,
    /// Higher land is lighter, lower land darker.
    pub terrain_depth: bool,
    /// Deep water is darker.
    pub transparency: bool,
    /// Lit as Xaero's: by the sky and the block light.
    pub lighting: bool,
    /// The sky's light now, 0 (midnight) to 15 (day).
    pub sky: f32,
}

impl Default for Shading {
    fn default() -> Self {
        Self {
            slopes: 2,
            terrain_depth: true,
            transparency: true,
            lighting: true,
            sky: 15.0,
        }
    }
}

/// A chunk's 256 columns as the maps show them, all zero where unseen.
pub type ShadedChunk = [[u8; 4]; 256];

/// Chunk `chunk` of `store`, shaded.
fn shade_chunk(store: &mut Store, chunk: (i32, i32), how: Shading) -> Rc<ShadedChunk> {
    let mut out = [[0u8; 4]; 256];
    let mut column = |x: i32, z: i32| store.column(x, z);
    for z in 0..16 {
        for x in 0..16 {
            if let Some(rgba) = shaded(&mut column, chunk.0 * 16 + x, chunk.1 * 16 + z, 1, how) {
                out[(z * 16 + x) as usize] = rgba;
            }
        }
    }
    Rc::new(out)
}

/// A column as the maps show it: land lit by its slope against the
/// columns `step` blocks north and north west of it (lighter facing north,
/// darker facing away) and darker below sea level; water as blended when
/// read; cut walls dark; all of it lit by the sky or the block light `light`
/// over it, whichever is more.
#[allow(clippy::too_many_arguments)]
pub fn shade(
    color: u32,
    height: i16,
    north: Option<i16>,
    north_west: Option<i16>,
    step: i32,
    light: u8,
    how: Shading,
) -> [u8; 4] {
    let rgb = color & RGB;
    let shadow = slope_light(height, north, north_west, step);
    let k = [0.0, 0.5, 1.0][how.slopes.min(2)];
    let slope = 1.0 + (shadow - 1.0) * k;
    let f = if color & WALL != 0 {
        [0.38; 3]
    } else if color & KIND == WATER {
        // `height` is the bottom's here: its relief shows through. Xaero's
        // shades the bottom under the water's tint, so red the most, blue
        // the least: measured at the edges of pools 1 to 10 deep (more over
        // sand, less over dirt, less the deeper). How deep it is was blended
        // in when the column was read.
        if how.transparency { [0.58, 0.38, 0.19].map(|k| 1.0 + (slope - 1.0) * k) } else { [1.0; 3] }
    } else {
        // Measured from Xaero's map at noon: land is one colour from sea
        // level up, and a little darker for each block below it.
        let below = (63 - i32::from(height)).max(0) as f32;
        let deep = if how.terrain_depth { (1.0 - below * 0.0145).max(0.7) } else { 1.0 };
        [slope * deep; 3]
    };
    // The colour grade from the blocks' texture averages to Xaero's,
    // measured on eighteen blocks of a test scene at noon: each channel
    // lands within 4 of Xaero's. Water is graded the same, its blend having
    // been fitted to it.
    let [_, r, g, b] = rgb.to_be_bytes().map(f32::from);
    let grade = [1.072, 1.102, 1.163];
    // Measured from Xaero's map at midnight, at dusk and at noon: each level
    // of light, sky or block, is a 24th, from 9 24ths in the dark, over the
    // colour as it is by day (white snow stays an even grey).
    let lit = if how.lighting { (9.0 + how.sky.max(f32::from(light.min(15)))) / 24.0 } else { 1.0 };
    let ch = |c: f32, i: usize| ((c * grade[i] * f[i]).min(255.0) * lit).round().clamp(0.0, 255.0) as u8;
    [ch(r, 0), ch(g, 1), ch(b, 2), 255]
}

/// How a column `height` high is lit by its slope, 1 on the flat: Xaero's
/// "3D" terrain slopes, measured from its map. The surface's normal comes
/// from the columns `step` blocks north and north west of it (the same
/// height where unknown); the light comes from the north, 42.5 degrees up,
/// over an ambient 0.58. Within 0.035 of Xaero's over steps and ramps of 1
/// to 32 blocks.
pub fn slope_light(height: i16, north: Option<i16>, north_west: Option<i16>, step: i32) -> f32 {
    const AMBIENT: f32 = 0.58;
    const UP: f32 = 42.5 * std::f32::consts::PI / 180.0;
    let step = step.max(1) as f32;
    let v = f32::from(height - north.unwrap_or(height)) / step;
    let d = f32::from(height - north_west.unwrap_or(height)) / step;
    // The normal (v - d, 1, -v) against the light (0, cos UP, -sin UP).
    let along = (UP.cos() + v * UP.sin()) / ((v - d) * (v - d) + 1.0 + v * v).sqrt();
    AMBIENT + (1.0 - AMBIENT) * along.max(0.0) / UP.cos()
}

/// The colour of column `(x, z)` from `column`, shaded against the columns
/// `step` blocks north and north west of it, or None where nothing was seen.
fn shaded(
    column: &mut dyn FnMut(i32, i32) -> Option<Column>,
    x: i32,
    z: i32,
    step: i32,
    how: Shading,
) -> Option<[u8; 4]> {
    let (color, height, depth, light) = column(x, z)?;
    if color & KIND == 0 {
        return Some(UNKNOWN);
    }
    // Against land and water alike, water by the height of its bottom: as
    // Xaero's, where the shore is a step down to the bottom.
    let like = |c: &Column| c.0 & WALL == 0 && c.0 & KIND != 0;
    let level = |c: Column| if c.0 & KIND == WATER { c.1 - i16::from(c.2) } else { c.1 };
    let north = column(x, z - step).filter(like).map(level);
    let north_west = column(x - step, z - step).filter(like).map(level);
    Some(shade(color, level((color, height, depth, light)), north, north_west, step, light, how))
}

/// Chunk offsets at Chebyshev distance `ring`.
pub fn ring_offsets(ring: i32) -> impl Iterator<Item = (i32, i32)> {
    (-ring..=ring).flat_map(move |dz| {
        (-ring..=ring)
            .filter(move |&dx| dx.abs() == ring || dz.abs() == ring)
            .map(move |dx| (dx, dz))
    })
}

// ---- The store: map columns kept on disk ---------------------------------------

/// A region is 32 x 32 chunks, one file.
pub const REGION: i32 = 32;
const REGION_CHUNKS: usize = (REGION * REGION) as usize;
/// The most regions kept in memory (a megabyte each at most).
const MAX_REGIONS: usize = 160;
const MAGIC: &[u8; 4] = b"ATL3";
/// The format before block light, still read.
const MAGIC_2: &[u8; 4] = b"ATL2";
/// A chunk's bytes in a region file: colours, heights, depths, lights.
const TILE_BYTES: usize = 256 * 4 + 256 * 2 + 256 + 256;
const TILE_BYTES_2: usize = 256 * 4 + 256 * 2 + 256;

#[derive(Debug)]
pub struct Region {
    pub chunks: Vec<Option<Box<Tile>>>,
    pub dirty: bool,
    /// Changes whenever a chunk of it changes (or it is read again).
    pub revision: u64,
    used: u64,
}

impl Region {
    fn new(revision: u64) -> Self {
        Self {
            chunks: (0..REGION_CHUNKS).map(|_| None).collect(),
            dirty: false,
            revision,
            used: 0,
        }
    }
}

fn region_of(chunk: i32) -> i32 {
    chunk.div_euclid(REGION)
}

fn region_index(cx: i32, cz: i32) -> usize {
    (cz.rem_euclid(REGION) * REGION + cx.rem_euclid(REGION)) as usize
}

/// PackBits: runs of a byte as (-(n - 1), byte), other bytes as (n - 1, bytes).
pub fn pack(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 16);
    let mut i = 0;
    while i < data.len() {
        let mut run = 1;
        while i + run < data.len() && run < 128 && data[i + run] == data[i] {
            run += 1;
        }
        if run >= 3 {
            out.push((1 - run as i32) as i8 as u8);
            out.push(data[i]);
            i += run;
            continue;
        }
        let start = i;
        let mut len = 0;
        while i < data.len() && len < 128 {
            if i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2] {
                break;
            }
            i += 1;
            len += 1;
        }
        out.push((len - 1) as u8);
        out.extend_from_slice(&data[start..start + len]);
    }
    out
}

/// What `pack` wrote; None when it is cut short.
pub fn unpack(data: &[u8], expected: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0;
    while i < data.len() {
        let n = data[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            out.extend_from_slice(data.get(i..i + len)?);
            i += len;
        } else if n != -128 {
            let len = (1 - i32::from(n)) as usize;
            let b = *data.get(i)?;
            i += 1;
            out.extend(std::iter::repeat_n(b, len));
        }
    }
    Some(out)
}

/// A region as its file holds it: the magic, then packed bytes of a bitmap
/// of the chunks it has and each chunk's colours, heights (little endian)
/// and depths.
pub fn encode_region(region: &Region) -> Vec<u8> {
    let mut raw = vec![0u8; REGION_CHUNKS / 8];
    for (i, c) in region.chunks.iter().enumerate() {
        if c.is_some() {
            raw[i / 8] |= 1 << (i % 8);
        }
    }
    for tile in region.chunks.iter().flatten() {
        for c in tile.color {
            raw.extend_from_slice(&c.to_le_bytes());
        }
        for h in tile.height {
            raw.extend_from_slice(&h.to_le_bytes());
        }
        raw.extend_from_slice(&tile.depth);
        raw.extend_from_slice(&tile.light);
    }
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.extend(pack(&raw));
    out
}

pub fn decode_region(bytes: &[u8], revision: u64) -> Option<Region> {
    let size = match bytes.get(..4)? {
        m if m == MAGIC => TILE_BYTES,
        m if m == MAGIC_2 => TILE_BYTES_2,
        _ => return None,
    };
    let len = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let raw = unpack(bytes.get(8..)?, len)?;
    if raw.len() != len || len < REGION_CHUNKS / 8 {
        return None;
    }
    let mut region = Region::new(revision);
    let mut at = REGION_CHUNKS / 8;
    for i in 0..REGION_CHUNKS {
        if raw[i / 8] >> (i % 8) & 1 == 0 {
            continue;
        }
        let body = raw.get(at..at + size)?;
        at += size;
        let mut tile = Tile::blank(0);
        for (j, c) in tile.color.iter_mut().enumerate() {
            *c = u32::from_le_bytes(body[j * 4..j * 4 + 4].try_into().ok()?);
        }
        for (j, h) in tile.height.iter_mut().enumerate() {
            *h = i16::from_le_bytes([body[1024 + j * 2], body[1025 + j * 2]]);
        }
        tile.depth.copy_from_slice(&body[1536..1792]);
        if size == TILE_BYTES {
            tile.light.copy_from_slice(&body[1792..2048]);
        }
        region.chunks[i] = Some(Box::new(tile));
    }
    Some(region)
}

/// The map columns of one world (or server) and dimension, read from and
/// written to `dir` a region at a time.
#[derive(Debug, Default)]
pub struct Store {
    pub dir: Option<PathBuf>,
    regions: HashMap<(i32, i32), Region>,
    /// Regions with no file, so they aren't looked for again.
    missing: HashSet<(i32, i32)>,
    clock: u64,
    next_revision: u64,
}

impl Store {
    /// Switches to `dir` (None: nothing is kept), writing what changed first.
    #[cfg(test)]
    pub fn open(&mut self, dir: Option<PathBuf>) {
        if self.dir == dir {
            return;
        }
        self.save();
        *self = Store {
            dir,
            next_revision: self.next_revision,
            ..Store::default()
        };
    }

    fn revision(&mut self) -> u64 {
        self.next_revision += 1;
        self.next_revision
    }

    fn path(&self, r: (i32, i32)) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("r.{}.{}.atl", r.0, r.1)))
    }

    /// Region `r`, read from its file the first time; None when it has none.
    fn region(&mut self, r: (i32, i32)) -> Option<&mut Region> {
        self.clock += 1;
        if !self.regions.contains_key(&r) {
            if self.missing.contains(&r) {
                return None;
            }
            let revision = self.revision();
            let read = self
                .path(r)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| decode_region(&b, revision));
            let Some(region) = read else {
                self.missing.insert(r);
                return None;
            };
            self.trim();
            self.regions.insert(r, region);
        }
        let clock = self.clock;
        let region = self.regions.get_mut(&r)?;
        region.used = clock;
        Some(region)
    }

    /// Keeps chunk `(cx, cz)`'s tile; true when it changed.
    pub fn put(&mut self, cx: i32, cz: i32, tile: Tile) -> bool {
        let r = (region_of(cx), region_of(cz));
        if self.region(r).is_none() {
            self.missing.remove(&r);
            self.trim();
            let revision = self.revision();
            self.regions.insert(r, Region::new(revision));
        }
        let revision = self.revision();
        let Some(region) = self.region(r) else {
            return false;
        };
        let slot = &mut region.chunks[region_index(cx, cz)];
        if slot.as_deref() == Some(&tile) {
            return false;
        }
        *slot = Some(Box::new(tile));
        region.dirty = true;
        region.revision = revision;
        true
    }

    #[cfg(test)]
    pub fn has(&mut self, cx: i32, cz: i32) -> bool {
        self.region((region_of(cx), region_of(cz)))
            .is_some_and(|r| r.chunks[region_index(cx, cz)].is_some())
    }

    /// Column `(x, z)`, if its chunk was ever seen.
    pub fn column(&mut self, x: i32, z: i32) -> Option<Column> {
        let (cx, cz) = (x >> 4, z >> 4);
        let region = self.region((region_of(cx), region_of(cz)))?;
        region.chunks[region_index(cx, cz)]
            .as_ref()
            .map(|t| t.column(x, z))
    }

    /// A number that changes when any of the regions over blocks
    /// `x0..=x1`, `z0..=z1` does.
    pub fn stamp(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) -> u64 {
        let mut stamp = 0u64;
        for rz in region_of(z0 >> 4)..=region_of(z1 >> 4) {
            for rx in region_of(x0 >> 4)..=region_of(x1 >> 4) {
                let rev = self.region((rx, rz)).map_or(0, |r| r.revision);
                stamp = stamp.wrapping_mul(0x100_0000_01B3).wrapping_add(rev);
            }
        }
        stamp
    }

    /// Writes the regions that changed: true when all of them were. One
    /// that couldn't be written stays changed, so the next save tries again.
    pub fn save(&mut self) -> bool {
        let dirty: Vec<(i32, i32)> = self
            .regions
            .iter()
            .filter(|(_, r)| r.dirty)
            .map(|(k, _)| *k)
            .collect();
        let mut all = true;
        for r in dirty {
            all &= self.write(r).is_ok();
        }
        all
    }

    /// Writes region `r` to its file, and only then counts it as saved.
    /// Without a folder nothing is kept, so there is nothing to fail.
    fn write(&mut self, r: (i32, i32)) -> std::io::Result<()> {
        let Some(path) = self.path(r) else {
            if let Some(region) = self.regions.get_mut(&r) {
                region.dirty = false;
            }
            return Ok(());
        };
        let Some(region) = self.regions.get_mut(&r) else {
            return Ok(());
        };
        let bytes = encode_region(region);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Whole files only: a new file, then renamed over the old one.
        let tmp = path.with_extension("atl.tmp");
        let done = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, &path));
        if done.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        done?;
        region.dirty = false;
        Ok(())
    }

    /// Forgets the least recently used regions past `MAX_REGIONS`. A
    /// changed region is written first and kept if that fails, so no map is
    /// lost; then the store grows instead.
    fn trim(&mut self) {
        let mut unsaved = HashSet::new();
        while self.regions.len() >= MAX_REGIONS {
            let Some(oldest) = self
                .regions
                .iter()
                .filter(|(k, _)| !unsaved.contains(*k))
                .min_by_key(|(_, r)| r.used)
                .map(|(k, _)| *k)
            else {
                return;
            };
            if self.regions[&oldest].dirty && self.write(oldest).is_err() {
                unsaved.insert(oldest);
                continue;
            }
            self.regions.remove(&oldest);
        }
    }

    #[cfg(test)]
    pub fn regions_loaded(&self) -> usize {
        self.regions.len()
    }
}

// ---- The minimap ----------------------------------------------------------------

/// The side of a map texture slot in texels (the world map's images), and
/// the minimap's image until the screen's size is known.
pub const MINI: usize = 128;
/// The minimap's image at most: Horizon's mod image is 1024 square.
pub const MINI_MAX: usize = 1024;
/// A block on the minimap at zoom 1, in screen pixels, as Xaero draws it.
pub const PIXELS_PER_BLOCK: f32 = 1.0;

/// How the minimap's edge looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// A thin dark rim.
    Thin,
    /// A thicker two-tone frame.
    Frame,
    /// No rim at all.
    Bare,
}

/// The pages of the settings screen, as Xaero's Minimap groups them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    View,
    BlockMap,
    Radar,
    Overlay,
    Info,
    Waypoints,
    Misc,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Main => "Minimap Settings",
            Page::View => "View Settings",
            Page::BlockMap => "Block Map Settings",
            Page::Radar => "Entity Radar Settings",
            Page::Overlay => "Overlay Settings",
            Page::Info => "Information Settings",
            Page::Waypoints => "Waypoint Settings",
            Page::Misc => "Miscellaneous Settings",
        }
    }
}

/// What a setting's button is, and how its value reads. Values are whole
/// numbers: an index for `Choice`, `value / div` units for the others.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Toggle,
    /// (stored name, label) per value.
    Choice(&'static [(&'static str, &'static str)]),
    /// `zero` names one value (`Auto`, `OFF`).
    Slider {
        min: i32,
        max: i32,
        div: i32,
        unit: &'static str,
        zero: Option<(i32, &'static str)>,
    },
    /// A button stepping through `steps`; `/atlas` takes any value from the
    /// first to the last.
    Steps {
        steps: &'static [i32],
        div: i32,
        unit: &'static str,
    },
}

/// One setting of the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opt {
    /// Its name in the settings file and after `/atlas`.
    pub key: &'static str,
    pub label: &'static str,
    pub page: Page,
    pub kind: Kind,
}

/// A setting's field as the table's whole number.
trait Val: Copy {
    fn raw(self) -> i32;
    fn cook(v: i32) -> Self;
}
impl Val for bool {
    fn raw(self) -> i32 {
        i32::from(self)
    }
    fn cook(v: i32) -> Self {
        v != 0
    }
}
impl Val for i32 {
    fn raw(self) -> i32 {
        self
    }
    fn cook(v: i32) -> Self {
        v
    }
}
impl Val for usize {
    fn raw(self) -> i32 {
        self as i32
    }
    fn cook(v: i32) -> Self {
        v.max(0) as usize
    }
}
/// Hundredths.
impl Val for f32 {
    fn raw(self) -> i32 {
        (self * 100.0).round() as i32
    }
    fn cook(v: i32) -> Self {
        v as f32 / 100.0
    }
}
impl Val for Style {
    fn raw(self) -> i32 {
        match self {
            Style::Frame => 0,
            Style::Thin => 1,
            Style::Bare => 2,
        }
    }
    fn cook(v: i32) -> Self {
        match v {
            1 => Style::Thin,
            2 => Style::Bare,
            _ => Style::Frame,
        }
    }
}
impl Val for MapMode {
    fn raw(self) -> i32 {
        MapMode::ALL.iter().position(|m| *m == self).unwrap_or(0) as i32
    }
    fn cook(v: i32) -> Self {
        MapMode::ALL[(v.max(0) as usize).min(MapMode::ALL.len() - 1)]
    }
}

/// Declares the settings: the struct, its defaults and the table, from one
/// line each.
macro_rules! settings {
    ($($field:ident: $ty:ty = $default:expr, $key:literal, $label:literal, $page:ident, $kind:expr;)*) => {
        /// Everything the settings screen and `/atlas` set, kept in one file
        /// in the mod's folder.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Settings {
            $(pub $field: $ty,)*
        }
        impl Default for Settings {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }
        pub const OPTS: &[Opt] = &[
            $(Opt { key: $key, label: $label, page: Page::$page, kind: $kind },)*
        ];
        impl Settings {
            /// Option `i`'s value.
            pub fn raw(&self, i: usize) -> i32 {
                let mut n = 0;
                $(
                    if n == i {
                        return Val::raw(self.$field);
                    }
                    n += 1;
                )*
                let _ = n;
                0
            }
            /// Sets option `i`.
            pub fn put(&mut self, i: usize, v: i32) {
                let mut n = 0;
                $(
                    if n == i {
                        self.$field = Val::cook(v);
                        return;
                    }
                    n += 1;
                )*
                let _ = n;
            }
        }
    };
}

const SHAPES: &[(&str, &str)] = &[("square", "Square"), ("round", "Round")];
const STYLES: &[(&str, &str)] = &[("frame", "Default"), ("thin", "Thin"), ("none", "None")];
const ENLARGED_ZOOMS: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("0.5", "0.5"),
    ("1", "1"),
    ("2", "2"),
    ("4", "4"),
    ("8", "8"),
];
const FRAME_COLORS: &[(&str, &str)] = &[
    ("default", "Default"),
    ("black", "Black"),
    ("white", "White"),
    ("red", "Red"),
    ("green", "Green"),
    ("blue", "Blue"),
    ("gold", "Gold"),
    ("purple", "Purple"),
];
/// The frame's bands in colour `i` of `FRAME_COLORS`: middle, light, dark.
fn frame_bands(i: usize) -> (u32, u32, u32) {
    let c: u32 = [0x9C9C9C, 0x2A2A2A, 0xF0F0F0, 0xC03030, 0x30A040, 0x3050D0, 0xD0A020, 0x9040C0]
        .get(i)
        .copied()
        .unwrap_or(0x9C9C9C);
    let mix = |t: u32, k: f32| {
        let ch = |s: u32| {
            let v = (c >> s & 255) as f32;
            let w = (t >> s & 255) as f32;
            ((v + (w - v) * k).round() as u32) << s
        };
        0xFF00_0000 | ch(16) | ch(8) | ch(0)
    };
    (0xFF00_0000 | c, mix(0xFFFFFF, 0.55), mix(0x000000, 0.45))
}

const ICON_MODES: &[(&str, &str)] = &[("off", "OFF"), ("tab", "Player List"), ("always", "Always")];
const NAME_MODES: &[(&str, &str)] = &[("none", "No Names"), ("players", "Players"), ("all", "All")];
const MAIN_ENTITY: &[(&str, &str)] = &[("arrow", "Arrow"), ("crosshair", "Crosshair"), ("dot", "Dot")];
const MAIN_ARROW: usize = 0;
const ARROW_COLORS: &[(&str, &str)] = &[
    ("red", "Red"),
    ("white", "White"),
    ("yellow", "Yellow"),
    ("green", "Green"),
    ("blue", "Blue"),
];
const ARROW_RGB: [u32; 5] = [0xE03030, 0xF0F0F0, 0xFFD030, 0x40D050, 0x4080F0];
const COLOR_MODES: &[(&str, &str)] = &[("accurate", "Accurate"), ("vanilla", "Vanilla")];
const AUTO_CAVE: &[(&str, &str)] = &[("ceiling3", "3x3 Ceiling"), ("ceiling1", "1x1 Ceiling"), ("off", "OFF")];
// Full is Xaero's (the names stored stay as they were).
const SLOPES: &[(&str, &str)] = &[("off", "OFF"), ("normal", "Soft"), ("strong", "Full")];
const GRID_COLORS: &[(&str, &str)] = &[("off", "OFF"), ("black", "Black"), ("white", "White"), ("red", "Red")];
const GRID_RGB: [[u16; 3]; 4] = [[0, 0, 0], [0, 0, 0], [255, 255, 255], [230, 40, 40]];
const ALIGNMENTS: &[(&str, &str)] = &[("center", "Center"), ("left", "Left"), ("right", "Right")];
const NORTH_COLORS: &[(&str, &str)] = &[("default", "Default"), ("red", "Red"), ("yellow", "Yellow"), ("blue", "Blue")];
const NORTH_RGB: [u32; 4] = [INK, 0xFF5555, 0xFFFF55, 0x5599FF];
const DISTANCE_MODES: &[(&str, &str)] = &[("off", "OFF"), ("looking", "Looking At"), ("always", "Always")];
const MODES: &[(&str, &str)] = &[("auto", "Auto"), ("cave", "Cave"), ("layer", "Layer")];
const CORNER_CHOICES: &[(&str, &str)] = &[
    ("top_left", "Top Left"),
    ("top_right", "Top Right"),
    ("bottom_left", "Bottom Left"),
    ("bottom_right", "Bottom Right"),
];

settings! {
    minimap: bool = true, "minimap", "Minimap", Main, Kind::Toggle;
    corner: usize = 0, "corner", "Position", Main, Kind::Choice(CORNER_CHOICES);

    size: i32 = 25, "size", "Size", View,
        Kind::Slider { min: 10, max: 60, div: 1, unit: "%", zero: None };
    round: bool = false, "shape", "Shape", View, Kind::Choice(SHAPES);
    lock_north: bool = false, "lock_north", "Lock Minimap North", View, Kind::Toggle;
    zoom: f32 = 1.0, "zoom", "Blocks per Pixel", View,
        Kind::Steps { steps: &[25, 50, 100, 200, 400, 800, 1600], div: 100, unit: "" };
    lighting: bool = true, "lighting", "Lighting", View, Kind::Toggle;
    cave_zoom: i32 = 2, "cave_zoom", "Cave Mode Zoom In", View,
        Kind::Slider { min: 1, max: 4, div: 1, unit: "x", zero: None };
    enlarged_zoom: usize = 0, "enlarged_zoom", "Zoom Level When Enlarged", View, Kind::Choice(ENLARGED_ZOOMS);
    opacity: i32 = 100, "opacity", "Opacity", View,
        Kind::Slider { min: 10, max: 100, div: 1, unit: "", zero: None };
    undiscovered: i32 = 50, "undiscovered", "Undiscovered Opacity", View,
        Kind::Slider { min: 0, max: 100, div: 1, unit: "", zero: None };
    style: Style = Style::Frame, "style", "Frame Style", View, Kind::Choice(STYLES);
    frame_color: usize = 0, "frame_color", "Frame Color", View, Kind::Choice(FRAME_COLORS);
    center_enlarged: bool = false, "center_enlarged", "Center When Enlarged", View, Kind::Toggle;
    keep_rotation: bool = false, "keep_rotation", "Keep Rotation When Enlarged", View, Kind::Toggle;
    toggle_enlarged: bool = false, "toggle_enlarged", "Toggled Enlarged Minimap", View, Kind::Toggle;
    hide_under_screen: bool = true, "hide_under_screen", "Hide When Under Screen", View, Kind::Toggle;
    hide_under_f3: bool = true, "hide_under_f3", "Hide When Under F3", View, Kind::Toggle;

    mode: MapMode = MapMode::Auto, "mode", "Cave Mode", BlockMap, Kind::Choice(MODES);
    layer_y: i32 = 64, "layer", "Layer Mode Height", BlockMap,
        Kind::Slider { min: -64, max: 320, div: 1, unit: "", zero: None };
    color_mode: usize = 0, "color_mode", "Block Color Mode", BlockMap, Kind::Choice(COLOR_MODES);
    auto_cave: usize = 0, "auto_cave", "Auto Cave Mode", BlockMap, Kind::Choice(AUTO_CAVE);
    cave_nether: bool = true, "cave_nether", "Cave Mode in the Nether", BlockMap, Kind::Toggle;
    cave_depth: i32 = 16, "cave_depth", "Cave Mode Depth", BlockMap,
        Kind::Slider { min: 8, max: 64, div: 1, unit: "", zero: None };
    cave_timer: i32 = 1000, "cave_timer", "Cave Mode Toggle Timer", BlockMap,
        Kind::Slider { min: 0, max: 5000, div: 1, unit: " ms", zero: None };
    slopes: usize = 2, "slopes", "Terrain Slopes", BlockMap, Kind::Choice(SLOPES);
    terrain_depth: bool = true, "terrain_depth", "Terrain Depth", BlockMap, Kind::Toggle;
    transparency: bool = true, "transparency", "Block Transparency", BlockMap, Kind::Toggle;
    flowers: bool = true, "flowers", "Show Flowers", BlockMap, Kind::Toggle;
    plants: bool = false, "plants", "Show Grass and Plants", BlockMap, Kind::Toggle;
    leaves: bool = true, "leaves", "Show Tree Leaves", BlockMap, Kind::Toggle;
    stained_glass: bool = true, "stained_glass", "Display Stained Glass", BlockMap, Kind::Toggle;

    dots: bool = true, "dots", "Display Radar", Radar, Kind::Toggle;
    show_players: bool = true, "show_players", "Players", Radar, Kind::Toggle;
    show_mobs: bool = true, "show_mobs", "Mobs", Radar, Kind::Toggle;
    show_items: bool = true, "show_items", "Dropped Items", Radar, Kind::Toggle;
    show_others: bool = true, "show_others", "Other Entities", Radar, Kind::Toggle;
    dot_size: i32 = 2, "dot_size", "Entity Dot Size", Radar,
        Kind::Slider { min: 1, max: 6, div: 1, unit: "", zero: None };
    icons: usize = 1, "icons", "Display Icons", Radar, Kind::Choice(ICON_MODES);
    icon_size: f32 = 1.0, "icon_size", "Entity Icon Scale", Radar,
        Kind::Slider { min: 50, max: 300, div: 100, unit: "x", zero: None };
    names: usize = 0, "names", "Display Names", Radar, Kind::Choice(NAME_MODES);
    entity_limit: i32 = 1000, "entity_limit", "Entity Number", Radar,
        Kind::Slider { min: 10, max: 1000, div: 1, unit: "", zero: None };
    height_limit: i32 = 0, "height_limit", "Entity Height Limit", Radar,
        Kind::Slider { min: 0, max: 256, div: 1, unit: "", zero: Some((0, "OFF")) };
    depth_fade: bool = true, "depth_fade", "Show Entity Depth", Radar, Kind::Toggle;
    fade_from: i32 = 8, "fade_from", "Min Y Distance For Depth Fade", Radar,
        Kind::Slider { min: 0, max: 64, div: 1, unit: "", zero: None };
    entity_height: bool = false, "entity_height", "Display Entity Height", Radar, Kind::Toggle;
    main_entity: usize = 1, "main_entity", "Main Entity As", Radar, Kind::Choice(MAIN_ENTITY);
    arrow_scale: f32 = 1.0, "arrow_scale", "Arrow Scale", Radar,
        Kind::Slider { min: 50, max: 300, div: 100, unit: "x", zero: None };
    arrow_color: usize = 0, "arrow_color", "Arrow Color", Radar, Kind::Choice(ARROW_COLORS);
    arrow_opacity: i32 = 100, "arrow_opacity", "Arrow Opacity", Radar,
        Kind::Slider { min: 10, max: 100, div: 1, unit: "", zero: None };
    hide_invisible: bool = true, "hide_invisible", "Hide Invisible Entities", Radar, Kind::Toggle;

    chunk_grid: usize = 0, "chunk_grid", "Chunk Grid", Overlay, Kind::Choice(GRID_COLORS);
    grid_width: i32 = 1, "grid_width", "Chunk Grid Line Width", Overlay,
        Kind::Slider { min: 1, max: 4, div: 1, unit: "", zero: None };
    light_overlay: bool = false, "light_overlay", "Light Overlay", Overlay, Kind::Toggle;
    light_level: i32 = 1, "light_level", "Light Overlay Below Level", Overlay,
        Kind::Slider { min: 1, max: 15, div: 1, unit: "", zero: None };
    slime: bool = false, "slime", "Slime Chunks", Overlay, Kind::Toggle;
    portals: bool = false, "portals", "Portal Highlights", Overlay, Kind::Toggle;
    view_square: bool = false, "view_distance", "Server View Distance", Overlay, Kind::Toggle;
    new_chunks: bool = false, "new_chunks", "New Chunks", Overlay, Kind::Toggle;
    old_chunks: bool = false, "old_chunks", "Old Chunks", Overlay, Kind::Toggle;

    coords: bool = true, "coords", "Coordinates", Info, Kind::Toggle;
    biome: bool = true, "biome", "Biome", Info, Kind::Toggle;
    time: bool = true, "time", "Time of Day", Info, Kind::Toggle;
    facing: bool = false, "facing", "Facing", Info, Kind::Toggle;
    dimension_line: bool = false, "dimension", "Dimension", Info, Kind::Toggle;
    chunk_line: bool = false, "chunk", "Chunk", Info, Kind::Toggle;
    light_line: bool = false, "light", "Light Level", Info, Kind::Toggle;
    weather_line: bool = false, "weather", "Weather", Info, Kind::Toggle;
    info_bg: i32 = 40, "info_bg", "Info Display Background Opacity", Info,
        Kind::Slider { min: 0, max: 100, div: 1, unit: "", zero: None };
    info_align: usize = 0, "info_align", "Info Text Alignment", Info, Kind::Choice(ALIGNMENTS);

    info_scale: f32 = 0.7, "ui_scale", "UI Scale", Misc,
        Kind::Slider { min: 40, max: 150, div: 100, unit: "x", zero: None };

    on_map: bool = true, "on_map", "On-Map Waypoints", Waypoints, Kind::Toggle;
    labels: bool = true, "labels", "In-World Waypoints", Waypoints, Kind::Toggle;
    icon_scale: f32 = 1.0, "icon_scale", "In-World Waypoint Icon Scale", Waypoints,
        Kind::Slider { min: 50, max: 300, div: 100, unit: "x", zero: None };
    name_scale: f32 = 0.7, "name_scale", "Waypoint Name Text Scale", Waypoints,
        Kind::Slider { min: 30, max: 200, div: 100, unit: "x", zero: None };
    distance_scale: f32 = 0.7, "distance_scale", "Waypoint Distance Text Scale", Waypoints,
        Kind::Slider { min: 30, max: 200, div: 100, unit: "x", zero: None };
    distance_mode: usize = 1, "distance_mode", "Distance to WP", Waypoints, Kind::Choice(DISTANCE_MODES);
    list_distance: bool = true, "list_distance", "Waypoint Distance In Menu", Waypoints, Kind::Toggle;
    name_above: bool = true, "name_above", "WP Name Above Distance", Waypoints, Kind::Toggle;
    angle_v: i32 = 180, "angle_v", "WP Dist. Vertic. Angle", Waypoints,
        Kind::Slider { min: 1, max: 180, div: 1, unit: "", zero: None };
    angle_h: i32 = 10, "angle_h", "WP Dist. Horis. Angle", Waypoints,
        Kind::Slider { min: 1, max: 180, div: 1, unit: "", zero: None };
    render_all_sets: bool = false, "render_all_sets", "Render All Waypoint Sets", Waypoints, Kind::Toggle;
    deathpoints: bool = true, "deathpoints", "Deathpoints", Waypoints, Kind::Toggle;
    keep_deathpoints: bool = true, "keep_deathpoints", "Keep Old Deathpoints", Waypoints, Kind::Toggle;
    delete_reached: bool = true, "delete_reached", "Delete Reached Deathpoints", Waypoints, Kind::Toggle;
    cross_dim_tp: bool = true, "cross_dim_tp", "Cross-Dimensional TP", Waypoints, Kind::Toggle;
    ow_in_nether: bool = false, "ow_in_nether", "Overworld Waypoints in Nether", Waypoints, Kind::Toggle;
    new_to_bottom: bool = false, "new_to_bottom", "New Waypoints to Bottom", Waypoints, Kind::Toggle;
    hide_coords: bool = false, "hide_coords", "Hide Waypoint Coordinates", Waypoints, Kind::Toggle;
    max_render: i32 = 0, "max_render", "Max WP Render Dist.", Waypoints,
        Kind::Slider { min: 0, max: 10000, div: 1, unit: "m", zero: Some((0, "Unlimited")) };
    min_render: i32 = 0, "min_render", "Min WP Render Dist.", Waypoints,
        Kind::Slider { min: 0, max: 500, div: 1, unit: "m", zero: Some((0, "OFF")) };
    km_at: i32 = 10000, "km_at", "Convert Distance to KM at", Waypoints,
        Kind::Slider { min: 100, max: 100000, div: 1, unit: "m", zero: None };
    precision: i32 = 1, "precision", "Waypoint Distance Precision", Waypoints,
        Kind::Slider { min: 0, max: 3, div: 1, unit: "", zero: None };
    world_opacity: i32 = 80, "world_opacity", "Waypoint Opacity in World", Waypoints,
        Kind::Slider { min: 10, max: 100, div: 1, unit: "", zero: None };
    map_opacity: i32 = 90, "map_opacity", "Waypoint Opacity on Map", Waypoints,
        Kind::Slider { min: 10, max: 100, div: 1, unit: "", zero: None };
    map_scale: f32 = 1.0, "map_scale", "On-Map Waypoint Scale", Waypoints,
        Kind::Slider { min: 50, max: 300, div: 100, unit: "x", zero: None };
    compass: bool = true, "compass", "Compass", Waypoints, Kind::Toggle;
    compass_scale: f32 = 1.0, "compass_scale", "Compass Scale", Waypoints,
        Kind::Slider { min: 50, max: 200, div: 100, unit: "x", zero: None };
    north_color: usize = 0, "north_color", "North Compass Color", Waypoints, Kind::Choice(NORTH_COLORS);
}

/// What Options > Mods > Atlas shows: the main switches. The rest are on the
/// settings screen.
const QUICK: [&str; 5] = ["minimap", "mode", "dots", "labels", "lock_north"];

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

fn parse_flag(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// `v / div` without needless decimals.
fn number_text(v: i32, div: i32) -> String {
    if div <= 1 {
        return v.to_string();
    }
    let n = f64::from(v) / f64::from(div);
    let text = format!("{n:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl Opt {
    /// The table's index of setting `key`.
    pub fn index(key: &str) -> Option<usize> {
        OPTS.iter().position(|o| o.key == key)
    }

    /// Value `v` as the button shows it.
    pub fn shown(&self, v: i32) -> String {
        match self.kind {
            Kind::Toggle => on_off(v != 0).to_owned(),
            Kind::Choice(names) => names.get(v.max(0) as usize).map_or("?", |n| n.1).to_owned(),
            Kind::Slider { zero: Some((at, name)), .. } if at == v => name.to_owned(),
            Kind::Slider { div, unit, .. } | Kind::Steps { div, unit, .. } => {
                format!("{}{unit}", number_text(v, div))
            }
        }
    }

    /// The button's whole label.
    pub fn text(&self, v: i32) -> String {
        format!("{}: {}", self.label, self.shown(v))
    }

    /// Value `v` as the settings file and `/atlas` write it.
    pub fn stored(&self, v: i32) -> String {
        match self.kind {
            Kind::Toggle => on_off(v != 0).to_ascii_lowercase(),
            Kind::Choice(names) => names.get(v.max(0) as usize).map_or("", |n| n.0).to_owned(),
            Kind::Slider { div, .. } | Kind::Steps { div, .. } => number_text(v, div),
        }
    }

    /// What `text` stands for, or what this setting takes.
    pub fn parse(&self, text: &str) -> Result<i32, String> {
        let text = text.trim().trim_end_matches('%');
        let number = |min: i32, max: i32, div: i32| {
            let takes = format!(
                "{} takes {} to {}",
                self.key,
                number_text(min, div),
                number_text(max, div)
            );
            let v: f64 = text.parse().map_err(|_| takes.clone())?;
            let v = (v * f64::from(div)).round();
            if v < f64::from(min) || v > f64::from(max) {
                return Err(takes);
            }
            Ok(v as i32)
        };
        match self.kind {
            Kind::Toggle => parse_flag(text)
                .map(i32::from)
                .ok_or_else(|| format!("{} takes on or off", self.key)),
            Kind::Choice(names) => names
                .iter()
                .position(|n| n.0 == text.to_ascii_lowercase())
                .map(|i| i as i32)
                .ok_or_else(|| {
                    let all: Vec<&str> = names.iter().map(|n| n.0).collect();
                    format!("{} takes {}", self.key, all.join(", "))
                }),
            Kind::Slider { min, max, div, zero, .. } => {
                if let Some((at, name)) = zero
                    && name.eq_ignore_ascii_case(text)
                {
                    return Ok(at);
                }
                number(min, max, div)
            }
            Kind::Steps { steps, div, .. } => {
                number(steps[0], steps[steps.len() - 1], div)
            }
        }
    }

    /// The next value a click gives.
    pub fn next(&self, v: i32) -> i32 {
        match self.kind {
            Kind::Toggle => i32::from(v == 0),
            Kind::Choice(names) => (v + 1) % names.len() as i32,
            Kind::Slider { min, max, .. } => {
                if v >= max {
                    min
                } else {
                    v + 1
                }
            }
            Kind::Steps { steps, .. } => steps.iter().copied().find(|&s| s > v).unwrap_or(steps[0]),
        }
    }
}

impl Settings {
    /// From the settings file `get` reads; the defaults for what is missing
    /// or broken.
    pub fn read(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut s = Settings::default();
        for line in get(SETTINGS_FILE).unwrap_or_default().lines() {
            if let Some((key, value)) = line.split_once('=') {
                let _ = s.set(key.trim(), value.trim());
            }
        }
        s
    }

    /// The settings file's text: `key=value` lines.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, o) in OPTS.iter().enumerate() {
            out.push_str(&format!("{}={}\n", o.key, o.stored(self.raw(i))));
        }
        out
    }

    /// Sets `key` from text; the error says what it takes.
    pub fn set(&mut self, key: &str, v: &str) -> Result<(), String> {
        let i = Opt::index(key).ok_or_else(|| format!("no setting {key}"))?;
        let v = OPTS[i].parse(v)?;
        self.put(i, v);
        Ok(())
    }

    /// The buttons of Options > Mods > Atlas.
    pub fn labels(&self) -> Vec<String> {
        let mut out = vec!["All settings: press Y in the game".to_owned()];
        for key in QUICK {
            if let Some(i) = Opt::index(key) {
                out.push(OPTS[i].text(self.raw(i)));
            }
        }
        out
    }

    /// Button `i` of `labels` clicked.
    pub fn click(&mut self, i: usize) {
        let Some(at) = i.checked_sub(1).and_then(|k| QUICK.get(k)).and_then(|k| Opt::index(k)) else {
            return;
        };
        self.put(at, OPTS[at].next(self.raw(at)));
    }

    /// `/atlas <words>`: what to say back, and whether a setting changed.
    pub fn command(&mut self, args: &str) -> (String, bool) {
        let words: Vec<&str> = args.split_whitespace().collect();
        match words.as_slice() {
            [] | ["help"] => {
                let keys: Vec<&str> = OPTS.iter().map(|o| o.key).collect();
                (format!("/atlas <setting> <value>, or seed <seed>. Settings: {}", keys.join(" ")), false)
            }
            ["mode", m, y] => {
                let before = *self;
                match self.set("mode", m).and_then(|()| self.set("layer", y)) {
                    Ok(()) => (format!("Atlas: mode {m}, layer Y {y}"), true),
                    Err(e) => {
                        *self = before;
                        (format!("Atlas: {e}"), false)
                    }
                }
            }
            [key, value] => match self.set(key, value) {
                Ok(()) => (format!("Atlas: {key} {value}"), true),
                Err(e) => (format!("Atlas: {e}"), false),
            },
            [key] => match Opt::index(key) {
                Some(i) => (format!("Atlas: {key} is {}", OPTS[i].stored(self.raw(i))), false),
                None => (format!("Atlas: no setting {key} (/atlas help)"), false),
            },
            _ => ("Atlas: /atlas help lists the settings".into(), false),
        }
    }

    /// How the maps shade columns.
    pub fn shading(&self) -> Shading {
        Shading {
            slopes: self.slopes,
            terrain_depth: self.terrain_depth,
            transparency: self.transparency,
            lighting: self.lighting,
            ..Shading::default()
        }
    }

    /// The player's arrow, ARGB.
    pub fn arrow_argb(&self) -> u32 {
        let alpha = self.arrow_opacity.clamp(0, 100) as u32 * 255 / 100;
        alpha << 24 | ARROW_RGB[self.arrow_color.min(ARROW_RGB.len() - 1)]
    }

    /// The minimap turns with the player.
    pub fn rotate(&self) -> bool {
        !self.lock_north
    }
}

/// The world's directions (x, z) that point right and down on the minimap:
/// east and south north up, else turned so that where the player looks is up.
pub fn mini_axes(rotate: bool, yaw: f32) -> ([f64; 2], [f64; 2]) {
    if !rotate {
        return ([1.0, 0.0], [0.0, 1.0]);
    }
    let (s, c) = f64::from(yaw).to_radians().sin_cos();
    // Forward is (-sin, cos); right of it, seen from above, is (-cos, -sin).
    ([-c, -s], [s, -c])
}

/// Where the minimap's texels lie: the texel grid is fixed to the world in
/// the map's turned frame, `origin` is the grid cell of texel (0, 0) and
/// `shift` how far into texel space the shown area starts, so the player
/// stays in the middle of what shows (`shift + (n - 1) / 2`). The image is
/// `n` texels square, one a screen pixel, one more than it shows so a
/// fraction of a texel can scroll into view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniGrid {
    pub right: [f64; 2],
    pub down: [f64; 2],
    pub bpp: f64,
    pub origin: [i64; 2],
    pub shift: [f32; 2],
    pub n: usize,
}

/// The grid of an `n` texel image (`n` is even), `bpp` blocks a texel.
pub fn mini_grid(x: f64, z: f64, yaw: f32, rotate: bool, bpp: f32, n: usize) -> MiniGrid {
    let (right, down) = mini_axes(rotate, yaw);
    let bpp = f64::from(bpp);
    let a = (x * right[0] + z * right[1]) / bpp - 0.5;
    let b = (x * down[0] + z * down[1]) / bpp - 0.5;
    let (fa, fb) = (a.floor(), b.floor());
    let back = (n / 2) as i64 - 1;
    MiniGrid {
        right,
        down,
        bpp,
        origin: [fa as i64 - back, fb as i64 - back],
        shift: [(a - fa) as f32, (b - fb) as f32],
        n,
    }
}

impl MiniGrid {
    /// The texels that show across.
    pub fn shown(&self) -> f32 {
        self.n as f32 - 1.0
    }
    /// The block under texel `(i, j)`'s centre.
    pub fn block(&self, i: usize, j: usize) -> (i32, i32) {
        let la = (self.origin[0] + i as i64) as f64 + 0.5;
        let lb = (self.origin[1] + j as i64) as f64 + 0.5;
        let wx = (la * self.right[0] + lb * self.down[0]) * self.bpp;
        let wz = (la * self.right[1] + lb * self.down[1]) * self.bpp;
        (wx.floor() as i32, wz.floor() as i32)
    }
    /// A world offset from the player as GUI pixels from the minimap's
    /// centre, for a minimap `size` pixels wide.
    pub fn to_screen(&self, dx: f64, dz: f64, size: f32) -> [f32; 2] {
        let scale = f64::from(size / self.shown()) / self.bpp;
        [
            ((dx * self.right[0] + dz * self.right[1]) * scale) as f32,
            ((dx * self.down[0] + dz * self.down[1]) * scale) as f32,
        ]
    }
}

/// The minimap's image for `grid` into `out` (`n` x `n` RGBA), round or
/// square; `pixel` gives a block's colour, None where nothing was seen.
pub fn compose_minimap(
    grid: &MiniGrid,
    round: bool,
    undiscovered: i32,
    pixel: &mut dyn FnMut(i32, i32) -> Option<[u8; 4]>,
    out: &mut Vec<u8>,
) {
    let n = grid.n;
    out.resize(n * n * 4, 0);
    let unseen = [UNKNOWN[0], UNKNOWN[1], UNKNOWN[2], (undiscovered.clamp(0, 100) * 255 / 100) as u8];
    let half = f64::from(grid.shown()) / 2.0;
    let centre = [f64::from(grid.shift[0]) + half, f64::from(grid.shift[1]) + half];
    for j in 0..n {
        for i in 0..n {
            let o = (j * n + i) * 4;
            if round {
                let (u, v) = (i as f64 + 0.5 - centre[0], j as f64 + 0.5 - centre[1]);
                if (u * u + v * v).sqrt() > half {
                    out[o..o + 4].copy_from_slice(&[0; 4]);
                    continue;
                }
            }
            let (x, z) = grid.block(i, j);
            out[o..o + 4].copy_from_slice(&pixel(x, z).unwrap_or(unseen));
        }
    }
}

/// Whether chunk `(cx, cz)` of a world with seed `seed` is a slime chunk
/// (`WorldgenRandom.seedSlimeChunk(x, z, seed, 987234911).nextInt(10) == 0`).
pub fn is_slime_chunk(seed: i64, cx: i32, cz: i32) -> bool {
    let mixed = seed
        .wrapping_add(i64::from(cx.wrapping_mul(cx).wrapping_mul(4_987_142)))
        .wrapping_add(i64::from(cx.wrapping_mul(5_947_611)))
        .wrapping_add(i64::from(cz.wrapping_mul(cz)).wrapping_mul(4_392_871))
        .wrapping_add(i64::from(cz.wrapping_mul(389_711)))
        ^ 987_234_911;
    // java.util.Random: setSeed, then nextInt(10).
    const MUL: i64 = 0x5_DEEC_E66D;
    const MASK: i64 = (1 << 48) - 1;
    let mut state = (mixed ^ MUL) & MASK;
    let mut next31 = || {
        state = (state.wrapping_mul(MUL).wrapping_add(0xB)) & MASK;
        (state >> 17) as i32
    };
    let bound = 10;
    let mut u = next31();
    let mut r = u % bound;
    while u.wrapping_sub(r).wrapping_add(bound - 1) < 0 {
        u = next31();
        r = u % bound;
    }
    r == 0
}

/// Greens the slime chunks on a composed minimap image.
pub fn tint_slime(grid: &MiniGrid, round: bool, seed: i64, image: &mut [u8]) {
    let _ = round;
    let mut known: HashMap<(i32, i32), bool> = HashMap::new();
    let n = grid.n;
    for j in 0..n {
        for i in 0..n {
            let o = (j * n + i) * 4;
            if image.len() < o + 4 || image[o + 3] == 0 {
                continue;
            }
            let (x, z) = grid.block(i, j);
            let chunk = (x >> 4, z >> 4);
            if !*known.entry(chunk).or_insert_with(|| is_slime_chunk(seed, chunk.0, chunk.1)) {
                continue;
            }
            tint(image, o, [40, 200, 60]);
        }
    }
}

/// A portal highlight's colour: purple for a Nether portal (XaeroPlus's
/// default), teal for an End portal or gateway.
pub fn portal_rgb(kinds: u8) -> u32 {
    if kinds & NETHER_PORTAL != 0 { 0xA0_40FF } else { 0x30_E0C0 }
}

/// A new chunk red, an old one green, as XaeroPlus's.
pub fn new_chunk_rgb(mark: u8) -> u32 {
    if mark == NEW_CHUNK { 0xFF_0000 } else { 0x00_FF00 }
}

/// Tints each chunk `rgb` gives a colour on a composed minimap image.
pub fn tint_chunks(grid: &MiniGrid, image: &mut [u8], rgb: impl Fn((i32, i32)) -> Option<u32>) {
    let n = grid.n;
    for j in 0..n {
        for i in 0..n {
            let o = (j * n + i) * 4;
            if image.len() < o + 4 || image[o + 3] == 0 {
                continue;
            }
            let (x, z) = grid.block(i, j);
            if let Some(c) = rgb((x >> 4, z >> 4)) {
                tint(image, o, [(c >> 16) as u16, (c >> 8 & 255) as u16, (c & 255) as u16]);
            }
        }
    }
}

/// Tints the chunks holding portals on a composed minimap image.
pub fn tint_portals(grid: &MiniGrid, portals: &HashMap<(i32, i32), u8>, image: &mut [u8]) {
    tint_chunks(grid, image, |c| portals.get(&c).map(|k| portal_rgb(*k)));
}

/// Draws the edge of the square of chunks `radius` around chunk `centre`
/// (the server's view distance) on a composed minimap image.
pub fn tint_square(grid: &MiniGrid, centre: (i32, i32), radius: i32, image: &mut [u8]) {
    let (x0, z0) = ((centre.0 - radius) * 16, (centre.1 - radius) * 16);
    let (x1, z1) = ((centre.0 + radius + 1) * 16 - 1, (centre.1 + radius + 1) * 16 - 1);
    let w = (grid.bpp.ceil() as i32).clamp(1, 8);
    let n = grid.n;
    for j in 0..n {
        for i in 0..n {
            let o = (j * n + i) * 4;
            if image.len() < o + 4 || image[o + 3] == 0 {
                continue;
            }
            let (x, z) = grid.block(i, j);
            let inside = x >= x0 && x <= x1 && z >= z0 && z <= z1;
            let edge = x < x0 + w || x > x1 - w || z < z0 + w || z > z1 - w;
            if inside && edge {
                for k in 0..3 {
                    image[o + k] = 255;
                }
            }
        }
    }
}

/// Mixes `rgb` two fifths into texel `o` of an image.
fn tint(image: &mut [u8], o: usize, rgb: [u16; 3]) {
    for (k, c) in rgb.into_iter().enumerate() {
        image[o + k] = ((u16::from(image[o + k]) * 3 + c * 2) / 5) as u8;
    }
}

/// Draws the chunk borders on a composed minimap image, `width` texels wide
/// in colour `GRID_RGB[color]`.
pub fn tint_grid(grid: &MiniGrid, color: usize, width: i32, image: &mut [u8]) {
    let rgb = GRID_RGB[color.min(GRID_RGB.len() - 1)];
    let blocks = ((f64::from(width) * grid.bpp).ceil() as i32).clamp(1, 8);
    let n = grid.n;
    for j in 0..n {
        for i in 0..n {
            let o = (j * n + i) * 4;
            if image.len() < o + 4 || image[o + 3] == 0 {
                continue;
            }
            let (x, z) = grid.block(i, j);
            if (x & 15) < blocks || (z & 15) < blocks {
                tint(image, o, rgb);
            }
        }
    }
}

/// What the minimap's image was composed for, to skip composing it again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniKey {
    pub origin: [i64; 2],
    pub yaw_bits: u32,
    pub bpp_bits: u32,
    pub round: bool,
    pub rotate: bool,
    pub cut: Cut,
    /// The store's stamp over what the image shows.
    pub stamp: u64,
    /// The seed slime chunks were shown for.
    pub slime: Option<i64>,
    pub undiscovered: i32,
    pub n: usize,
    pub grid: (usize, i32),
    pub light: Option<i32>,
    /// The portals' revision, when they show.
    pub portals: Option<u64>,
    /// The new chunks' revision and which show (new, old).
    pub new_chunks: Option<(u64, bool, bool)>,
    /// The view distance square: the player's chunk and the radius.
    pub square: Option<(i32, i32, i32)>,
}

// ---- The world map -----------------------------------------------------------------

/// A world map image: 128 x 128 texels of `1 << level` blocks each, the
/// `x`-th and `z`-th of its level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub cut: Cut,
    pub level: u8,
    pub x: i32,
    pub z: i32,
}

pub const MAP_TEXELS: i32 = 128;
/// The deepest level: 8 blocks a texel.
pub const MAX_LEVEL: u8 = 3;
/// Blocks per GUI pixel, zoomed in and out the most.
pub const MIN_BPP: f64 = 1.0 / 8.0;
pub const MAX_BPP: f64 = 8.0;

impl TileKey {
    /// The blocks `[x0, z0, x1, z1]` (inclusive) the image covers.
    pub fn blocks(self) -> [i32; 4] {
        let span = MAP_TEXELS << self.level;
        let (x0, z0) = (self.x * span, self.z * span);
        [x0, z0, x0 + span - 1, z0 + span - 1]
    }
}

/// The level whose texels show at a GUI pixel or more at `bpp`.
pub fn map_level(bpp: f64) -> u8 {
    if bpp <= 1.0 {
        return 0;
    }
    (bpp.log2().ceil() as u8).min(MAX_LEVEL)
}

/// The world map image `key` from `store` into `out` (RGBA, transparent where
/// nothing was seen). Each texel shows the block at its centre.
pub fn compose_map_tile(store: &mut Store, key: TileKey, how: Shading, out: &mut Vec<u8>) {
    let n = 1i32 << key.level;
    let [x0, z0, ..] = key.blocks();
    out.clear();
    out.resize((MAP_TEXELS * MAP_TEXELS * 4) as usize, 0);
    let mut column = |x: i32, z: i32| store.column(x, z);
    for j in 0..MAP_TEXELS {
        for i in 0..MAP_TEXELS {
            let (x, z) = (x0 + i * n + n / 2, z0 + j * n + n / 2);
            if let Some(rgba) = shaded(&mut column, x, z, n, how) {
                let o = ((j * MAP_TEXELS + i) * 4) as usize;
                out[o..o + 4].copy_from_slice(&rgba);
            }
        }
    }
}

/// Where the world map looks: the block at the screen's centre and the
/// blocks per GUI pixel. It follows the player until dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct MapView {
    pub centre: [f64; 2],
    pub bpp: f64,
    pub follow: bool,
    /// A left drag in progress, and whether it moved (so a click on a
    /// waypoint isn't a drag).
    pub dragging: bool,
    pub dragged: f32,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            centre: [0.0, 0.0],
            bpp: 1.0,
            follow: true,
            dragging: false,
            dragged: 0.0,
        }
    }
}

impl MapView {
    /// The block under GUI pixel `at` of a `size` screen.
    pub fn world(&self, at: [f32; 2], size: [f32; 2]) -> [f64; 2] {
        [
            self.centre[0] + f64::from(at[0] - size[0] / 2.0) * self.bpp,
            self.centre[1] + f64::from(at[1] - size[1] / 2.0) * self.bpp,
        ]
    }
    /// The GUI pixel of world point `(x, z)`.
    pub fn screen(&self, x: f64, z: f64, size: [f32; 2]) -> [f32; 2] {
        [
            size[0] / 2.0 + ((x - self.centre[0]) / self.bpp) as f32,
            size[1] / 2.0 + ((z - self.centre[1]) / self.bpp) as f32,
        ]
    }
    /// Zooms by `steps` wheel notches (up zooms in), keeping the block under
    /// `at` where it is.
    pub fn zoom(&mut self, steps: f64, at: [f32; 2], size: [f32; 2]) {
        let before = self.world(at, size);
        // Powers of two: every block a whole number of screen pixels.
        let notch = if steps > 0.0 { 0.5 } else { 2.0 };
        self.bpp = (self.bpp * notch).clamp(MIN_BPP, MAX_BPP);
        let after = self.world(at, size);
        self.centre[0] += before[0] - after[0];
        self.centre[1] += before[1] - after[1];
    }
    pub fn pan(&mut self, delta: [f32; 2]) {
        self.centre[0] -= f64::from(delta[0]) * self.bpp;
        self.centre[1] -= f64::from(delta[1]) * self.bpp;
        self.follow = false;
    }
    /// The images a `size` screen shows, with where each goes.
    pub fn tiles(&self, size: [f32; 2], cut: Cut) -> Vec<(TileKey, [f32; 4])> {
        let level = map_level(self.bpp);
        let span = f64::from(MAP_TEXELS << level);
        let [x0, z0] = self.world([0.0, 0.0], size);
        let [x1, z1] = self.world(size, size);
        let mut out = Vec::new();
        for tz in (z0 / span).floor() as i32..=(z1 / span).floor() as i32 {
            for tx in (x0 / span).floor() as i32..=(x1 / span).floor() as i32 {
                let key = TileKey { cut, level, x: tx, z: tz };
                // Edges rounded the same way for both neighbours: no seams.
                let a = self.screen(f64::from(tx) * span, f64::from(tz) * span, size).map(f32::round);
                let b = self
                    .screen(f64::from(tx + 1) * span, f64::from(tz + 1) * span, size)
                    .map(f32::round);
                out.push((key, [a[0], a[1], b[0] - a[0], b[1] - a[1]]));
            }
        }
        out
    }
}

/// The world map's images in the game's map texture slots: which slot key
/// each was given (negative, so they never meet a real map's id), what of
/// the store it showed, and the slot it is in once uploaded.
#[derive(Debug, Default)]
pub struct MapImages {
    ids: HashMap<TileKey, i32>,
    next: i32,
    pub stamps: HashMap<TileKey, u64>,
    pub slots: HashMap<TileKey, usize>,
}

/// The mob face sheet's slot key: far below the world map's.
pub const FACES_SLOT_KEY: i32 = i32::MIN + 8;

impl MapImages {
    pub fn clear(&mut self) {
        let next = self.next;
        *self = MapImages {
            next,
            ..MapImages::default()
        };
    }
    /// The map slot key image `key` uses.
    pub fn id(&mut self, key: TileKey) -> i32 {
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        self.next = self.next.checked_sub(1).filter(|n| *n > i32::MIN + 1024).unwrap_or(-1);
        let id = self.next;
        // A key whose id was handed out again loses it.
        self.forget_id(id);
        self.ids.insert(key, id);
        id
    }
    /// The image with slot key `id` lost its slot (another image took it).
    pub fn forget_id(&mut self, id: i32) {
        let gone: Vec<TileKey> = self.ids.iter().filter(|(_, v)| **v == id).map(|(k, _)| *k).collect();
        for k in gone {
            self.ids.remove(&k);
            self.stamps.remove(&k);
            self.slots.remove(&k);
        }
    }
}

/// Kinds of portal a chunk holds (`Maps::portals`), a bit each.
pub const NETHER_PORTAL: u8 = 1;
pub const END_PORTAL: u8 = 2;
pub const END_GATEWAY: u8 = 4;

/// A chunk's mark in `Maps::new_chunks`: generated when it came (its
/// palettes as the generator left them), or loaded from the server's saves.
pub const NEW_CHUNK: u8 = 1;
pub const OLD_CHUNK: u8 = 2;

/// Whether a section's block palette is as the generator left it, never
/// saved by the server (XaeroPlus's PaletteNewChunks, MIT, rfresh2; the idea
/// from etianl's Trouser-Streak). A server saving a chunk rebuilds each
/// palette from the blocks present, in the order they are met; a new chunk
/// is sent before it is saved. So a small (linear, up to 4 bits) palette
/// out of that order, or a larger (hash map) one with entries no block
/// uses, was never saved. `bits`, `palette` and `words` are as sent.
pub fn fresh_palette(bits: u8, palette: &[u32], words: &[u64]) -> bool {
    if bits == 0 || palette.len() < 2 || bits > 8 {
        return false;
    }
    let per = 64 / usize::from(bits);
    let mask = (1u64 << bits) - 1;
    let index = |i: usize| words.get(i / per).map_or(0, |w| (w >> ((i % per) * usize::from(bits)) & mask) as usize);
    let mut seen = vec![false; palette.len()];
    if bits <= 4 {
        // Linear: entries first met in palette order, else fresh.
        let mut next = 0;
        for i in 0..4096 {
            let v = index(i);
            if v >= seen.len() || seen[v] {
                continue;
            }
            if v != next {
                return true;
            }
            seen[v] = true;
            next += 1;
            if next == seen.len() {
                return false;
            }
        }
        return false;
    }
    // Hash map: an entry nothing uses.
    let mut used = 0;
    for i in 0..4096 {
        let v = index(i);
        if v < seen.len() && !seen[v] {
            seen[v] = true;
            used += 1;
            if used == seen.len() {
                return false;
            }
        }
    }
    true
}

/// Whether a chunk was new when it came, from its sections' block palettes
/// as sent (`bits`, palette, words): three fresh sections at least, as
/// XaeroPlus counts (a server may tick a chunk before sending it, which
/// can leave one section looking fresh).
pub fn is_new_chunk<'a>(sections: impl IntoIterator<Item = (u8, &'a [u32], &'a [u64])>) -> bool {
    sections
        .into_iter()
        .filter(|(bits, palette, words)| fresh_palette(*bits, palette, words))
        .nth(2)
        .is_some()
}

/// Marks on chunks, a byte each: the portals seen in them (kinds of
/// portal), or whether they were new when they came. Kept per dimension in
/// its map folder, as XaeroPlus's highlights are.
#[derive(Debug, Default)]
pub struct ChunkMarks {
    pub chunks: HashMap<(i32, i32), u8>,
    /// Changes when a chunk is added or dropped.
    pub revision: u64,
    dirty: bool,
}

const PORTALS_FILE: &str = "portals.txt";
const NEW_CHUNKS_FILE: &str = "newchunks.txt";

impl ChunkMarks {
    /// `cx cz kinds` a line.
    pub fn decode(text: &str) -> Self {
        let chunks = text
            .lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace().map(str::parse::<i32>);
                let (cx, cz, k) = (f.next()?.ok()?, f.next()?.ok()?, f.next()?.ok()?);
                let k = u8::try_from(k).ok().filter(|k| *k != 0)?;
                Some(((cx, cz), k))
            })
            .collect();
        ChunkMarks { chunks, ..ChunkMarks::default() }
    }

    pub fn encode(&self) -> String {
        let mut rows: Vec<_> = self.chunks.iter().collect();
        rows.sort();
        rows.iter().map(|((x, z), k)| format!("{x} {z} {k}\n")).collect()
    }

    /// Writes them to `file` in `dir` when they changed: false when that
    /// failed (they stay changed, to be tried again).
    fn save(&mut self, dir: &std::path::Path, file: &str) -> bool {
        if !self.dirty {
            return true;
        }
        let tmp = dir.join(format!("{file}.tmp"));
        let done = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&tmp, self.encode()))
            .and_then(|()| std::fs::rename(&tmp, dir.join(file)));
        if done.is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        self.dirty = false;
        true
    }

    /// Chunk `chunk` was scanned and holds `kinds` (0: none): true when that
    /// changed what is known.
    pub fn put(&mut self, chunk: (i32, i32), kinds: u8) -> bool {
        let before = self.chunks.get(&chunk).copied().unwrap_or(0);
        if before == kinds {
            return false;
        }
        if kinds == 0 {
            self.chunks.remove(&chunk);
        } else {
            self.chunks.insert(chunk, kinds);
        }
        self.revision += 1;
        self.dirty = true;
        true
    }
}

/// The map columns of every cut of one world's dimension, each cut in its
/// own folder under `dir`, and the portals seen there.
#[derive(Debug, Default)]
pub struct Maps {
    pub dir: Option<PathBuf>,
    stores: HashMap<Cut, Store>,
    next_revision: u64,
    pub portals: ChunkMarks,
    /// New (`NEW_CHUNK`) and old (`OLD_CHUNK`) chunks.
    pub new_chunks: ChunkMarks,
}

/// The most cuts kept open at once.
const MAX_CUTS: usize = 4;

impl Maps {
    /// Switches to `dir` (None: nothing is kept), writing what changed first.
    pub fn open(&mut self, dir: Option<PathBuf>) {
        if self.dir == dir {
            return;
        }
        self.save();
        self.stores.clear();
        let read = |file: &str, revision: u64| {
            let mut marks = dir
                .as_ref()
                .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
                .map(|t| ChunkMarks::decode(&t))
                .unwrap_or_default();
            // Never the same revision as before: what is drawn follows.
            marks.revision = revision + 1;
            marks
        };
        self.portals = read(PORTALS_FILE, self.portals.revision);
        self.new_chunks = read(NEW_CHUNKS_FILE, self.new_chunks.revision);
        self.dir = dir;
    }

    /// The cut folder of this dimension holding the most map, else the
    /// surface: what a map of a dimension the player isn't in shows.
    pub fn fullest_cut(&self) -> Cut {
        let Some(dir) = &self.dir else {
            return Cut::Surface;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Cut::Surface;
        };
        entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let cut = if name == "surface" {
                    Cut::Surface
                } else {
                    Cut::Below(name.strip_prefix("below")?.parse().ok()?)
                };
                let size: u64 = std::fs::read_dir(e.path()).ok()?.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum();
                Some((size, cut))
            })
            .max_by_key(|(size, cut)| (*size, *cut == Cut::Surface))
            .map_or(Cut::Surface, |(_, cut)| cut)
    }
    /// The store of `cut`, opened the first time.
    pub fn store(&mut self, cut: Cut) -> &mut Store {
        if !self.stores.contains_key(&cut) {
            if self.stores.len() >= MAX_CUTS {
                // Close one other cut (Surface last).
                // One that can't be written stays open, so its map isn't lost.
                let mut others: Vec<Cut> = self.stores.keys().copied().filter(|c| *c != cut).collect();
                others.sort_by_key(|c| *c == Cut::Surface);
                if let Some(other) = others.into_iter().find(|c| self.stores.get_mut(c).is_some_and(Store::save))
                    && let Some(st) = self.stores.remove(&other)
                {
                    self.next_revision = self.next_revision.max(st.next_revision);
                }
            }
            let mut st = Store {
                next_revision: self.next_revision,
                ..Store::default()
            };
            st.dir = self.dir.as_ref().map(|d| d.join(cut.folder()));
            self.stores.insert(cut, st);
        }
        let st = self.stores.get_mut(&cut).expect("just inserted");
        // Revisions keep growing across cuts, so a stamp never repeats.
        self.next_revision = self.next_revision.max(st.next_revision);
        st
    }
    /// Writes what changed of every cut, and the portals: true when all of
    /// it was.
    pub fn save(&mut self) -> bool {
        let mut all = true;
        for st in self.stores.values_mut() {
            all &= st.save();
        }
        if let Some(dir) = &self.dir {
            for (marks, file) in [(&mut self.portals, PORTALS_FILE), (&mut self.new_chunks, NEW_CHUNKS_FILE)] {
                all &= marks.save(dir, file);
            }
        }
        all
    }
}

// ---- State -----------------------------------------------------------------------

/// Everything the mod keeps, on the window's thread.
#[derive(Debug)]
pub struct State {
    /// The world or server the waypoints and maps belong to, and the folder
    /// the waypoints are kept in.
    pub world: Option<String>,
    pub dir: Option<PathBuf>,
    pub waypoints: Vec<Waypoint>,
    /// The world's seed, as the player typed it (`/atlas seed`), for slime
    /// chunks: a server never sends it.
    pub seed: Option<i64>,
    pub mode: Mode,
    pub filter: Filter,
    pub scroll: usize,
    /// The waypoint picked in the list, whose buttons show below it.
    pub selected: Option<usize>,
    /// The row whose Delete was clicked once and asks to be clicked again.
    pub confirm_delete: Option<usize>,
    /// The character of the key that opened the screen, which arrives right
    /// after the screen opened and must not be typed into it.
    pub skip_char: Option<char>,
    pub close_requested: bool,
    /// Options > Mods > Atlas and `/atlas`, read once.
    pub settings: Option<Settings>,
    /// Changed on the mod's own screen, not written yet.
    pub settings_changed: bool,
    /// The slider being dragged: its option and rectangle.
    pub slider: Option<(usize, [f32; 4])>,
    /// Whether the player may teleport (an operator), as the game last said.
    pub can_teleport: bool,
    /// A teleport asked for: the dimension and block, sent by the next frame.
    pub teleport: Option<(String, [i32; 3])>,
    /// A chat line asked for (Share), sent by the next frame.
    pub say: Option<String>,
    /// The set shown and new waypoints go in.
    pub set: String,
    /// The list's filter text, and whether typing goes there.
    pub list_filter: String,
    pub filter_focus: bool,
    pub confirm_clear: bool,
    /// The player was dead last frame.
    pub was_dead: bool,
    /// Map columns of every cut seen, kept on disk.
    pub maps: Maps,
    /// When each loaded chunk was last scanned for a cut, and the world's
    /// revision then.
    pub scans: HashMap<(Cut, (i32, i32)), (u64, Instant)>,
    /// The dimension the maps and the scans are of.
    pub map_dimension: Option<String>,
    /// What both maps show now.
    pub cut: Cut,
    /// A change of cut waiting for the toggle timer.
    pub pending_cut: Option<(Cut, Instant)>,
    pub mini_key: Option<MiniKey>,
    pub mini_grid: Option<MiniGrid>,
    pub mini_image: Vec<u8>,
    /// The image's side in texels: the minimap's in screen pixels, plus one.
    pub mini_n: usize,
    /// Screen pixels a GUI pixel.
    pub gui_scale: f32,
    /// The image has been uploaded.
    pub mini_ready: bool,
    pub mini_uploaded: Option<Instant>,
    /// Shaded chunks of `shade_cut`, for composing the image every frame.
    pub shade_cache: HashMap<(i32, i32), Rc<ShadedChunk>>,
    pub shade_cut: Option<Cut>,
    /// The sky's light the maps are shaded for (`sky_light`).
    pub sky: f32,
    /// The enlarge key is held, or toggled on.
    pub enlarged: bool,
    /// The player list key is held: icons show.
    pub tab_held: bool,
    /// Where each mob's face is in the face sheet, and its texture slot.
    pub faces: HashMap<String, [f32; 4]>,
    pub face_slot: Option<usize>,
    pub map: MapView,
    pub images: MapImages,
    /// The size the mod's screen was last drawn at, and the pointer then.
    pub screen_size: [f32; 2],
    pub mouse: [f32; 2],
    pub last_save: Option<Instant>,
    /// The camera this frame is drawn from: position, yaw, pitch.
    pub frame_camera: Option<([f64; 3], f32, f32)>,
    /// The dimension the world map shows when it isn't the player's, its
    /// maps (only read), and the cut shown of them.
    pub map_shown: Option<String>,
    pub other_maps: Maps,
    pub other_cut: Cut,
    /// The server's view distance in chunks, as it last said.
    pub view_distance: Option<i32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            world: None,
            dir: None,
            waypoints: Vec::new(),
            seed: None,
            mode: Mode::Closed,
            filter: Filter::All,
            scroll: 0,
            selected: None,
            confirm_delete: None,
            skip_char: None,
            close_requested: false,
            settings: None,
            settings_changed: false,
            slider: None,
            can_teleport: false,
            teleport: None,
            say: None,
            set: DEFAULT_SET.to_owned(),
            list_filter: String::new(),
            filter_focus: false,
            confirm_clear: false,
            was_dead: false,
            maps: Maps::default(),
            scans: HashMap::new(),
            map_dimension: None,
            cut: Cut::Surface,
            pending_cut: None,
            mini_key: None,
            mini_grid: None,
            mini_image: Vec::new(),
            mini_n: MINI,
            gui_scale: 2.0,
            mini_ready: false,
            mini_uploaded: None,
            shade_cache: HashMap::new(),
            shade_cut: None,
            sky: 15.0,
            enlarged: false,
            tab_held: false,
            faces: HashMap::new(),
            face_slot: None,
            map: MapView::default(),
            images: MapImages::default(),
            screen_size: [480.0, 270.0],
            mouse: [0.0; 2],
            last_save: None,
            frame_camera: None,
            map_shown: None,
            other_maps: Maps::default(),
            other_cut: Cut::Surface,
            view_distance: None,
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Runs `f` on the mod's state. Hooks call it without calling back into the
/// game inside, so it is never borrowed twice.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// The letter a key name types (`key.keyboard.n` types `n`), if it is one.
pub fn key_char(key: &str) -> Option<char> {
    let last = key.strip_prefix("key.keyboard.")?;
    let mut chars = last.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

// ---- Storage ---------------------------------------------------------------

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// One line per waypoint: name, x, y, z, dimension, colour, hidden, set and
/// kind. Temporary waypoints aren't kept.
pub fn encode(list: &[Waypoint]) -> String {
    let mut out = String::from("# waypoints 1\n");
    for w in list.iter().filter(|w| w.kind != WpKind::Temporary) {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{:06X}\t{}\t{}\t{}\n",
            escape(&w.name),
            w.pos[0],
            w.pos[1],
            w.pos[2],
            w.dimension,
            w.color & 0xFF_FFFF,
            u8::from(w.hidden),
            escape(&w.set),
            if w.kind == WpKind::Death { "d" } else { "w" }
        ));
    }
    out
}

/// What `encode` wrote; broken lines are skipped.
pub fn decode(text: &str) -> Vec<Waypoint> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            Some(Waypoint {
                name: unescape(f[0]),
                pos: [f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?],
                dimension: f[4].to_owned(),
                color: u32::from_str_radix(f[5], 16).ok()? & 0xFF_FFFF,
                hidden: f[6].trim() == "1",
                set: f.get(7).map_or(DEFAULT_SET.to_owned(), |t| unescape(t.trim())),
                kind: if f.get(8).is_some_and(|k| k.trim() == "d") { WpKind::Death } else { WpKind::Normal },
            })
        })
        .collect()
}

/// A key or id as a plain file name.
pub fn safe_name(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The file a world's waypoints are kept in, named after its key.
pub fn world_file(key: &str) -> String {
    format!("{}.txt", safe_name(key))
}

/// The file a world's seed is kept in.
pub fn seed_file(key: &str) -> String {
    format!("{}.seed", safe_name(key))
}

/// A seed as the create-world screen reads it: a number, else the text's
/// `String.hashCode()`.
pub fn parse_seed(text: &str) -> Option<i64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(n) = t.parse::<i64>() {
        return Some(n);
    }
    let hash = t.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
    Some(i64::from(hash))
}

/// The folder a world's map of `dimension` is kept in, under the mod's.
pub fn map_dir(data: &std::path::Path, key: &str, dimension: &str) -> PathBuf {
    data.join("maps").join(safe_name(key)).join(safe_name(dimension))
}

// ---- Words -----------------------------------------------------------------

pub fn dimension_label(id: &str) -> String {
    match id {
        OVERWORLD => "Overworld".into(),
        NETHER => "Nether".into(),
        END => "The End".into(),
        other => other.rsplit(':').next().unwrap_or(other).replace('_', " "),
    }
}

pub fn dimension_color(id: &str) -> u32 {
    match id {
        OVERWORLD => 0x55FF55,
        NETHER => 0xFF5555,
        END => 0xD27BFF,
        _ => MUTED,
    }
}

fn next_dimension(id: &str) -> &'static str {
    match id {
        OVERWORLD => NETHER,
        NETHER => END,
        _ => OVERWORLD,
    }
}

/// Where yaw `yaw` looks: `South`, `North West`.
pub fn facing_text(yaw: f32) -> String {
    let names = ["South", "South West", "West", "North West", "North", "North East", "East", "South East"];
    let i = ((yaw.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8;
    names[i].to_owned()
}

/// The sky's light at day time `ticks`, 0 at midnight to 15 by day, in
/// half levels: the game's sky curve, which Xaero's map follows (measured
/// at noon, dusk and midnight; at time 12500 Xaero's is half a level
/// darker than this).
pub fn sky_light(ticks: i64) -> f32 {
    use std::f32::consts::PI;
    let f = (ticks.rem_euclid(24_000) as f32 / 24_000.0 - 0.25).rem_euclid(1.0);
    let angle = (f * 2.0 + (0.5 - (f * PI).cos() / 2.0)) / 3.0;
    let s = ((angle * 2.0 * PI).cos() * 2.0 + 0.2).clamp(0.0, 1.0);
    (s * 30.0).round() / 2.0
}

/// `minecraft:dark_forest` as `Dark Forest`.
pub fn biome_label(id: &str) -> String {
    let name = id.rsplit(':').next().unwrap_or(id);
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The world's time of day `ticks` as `Day 3, 14:30` (tick 0 is 6:00 of day 1).
pub fn time_text(ticks: i64) -> String {
    let day = ticks.div_euclid(24_000) + 1;
    let t = ticks.rem_euclid(24_000);
    let hours = (t / 1000 + 6) % 24;
    let minutes = t % 1000 * 60 / 1000;
    format!("Day {day}, {hours:02}:{minutes:02}")
}

/// `52.3m`, in kilometres from `km_at` metres on, to `precision` decimals.
pub fn distance_text(d: f64, km_at: i32, precision: i32) -> String {
    let p = precision.clamp(0, 3) as usize;
    if d >= f64::from(km_at) {
        format!("{:.p$}km", d / 1000.0)
    } else {
        format!("{d:.p$}m")
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

/// The waypoint's block centre.
fn centre(w: &Waypoint) -> [f64; 3] {
    [
        f64::from(w.pos[0]) + 0.5,
        f64::from(w.pos[1]) + 0.5,
        f64::from(w.pos[2]) + 0.5,
    ]
}

/// Blocks of `to` a block of `from` spans across: 8 from the Overworld to
/// the Nether, 1/8 back, None between the End and the rest.
fn dimension_scale(from: &str, to: &str) -> Option<f64> {
    match (from, to) {
        _ if from == to => Some(1.0),
        (OVERWORLD, NETHER) => Some(1.0 / 8.0),
        (NETHER, OVERWORLD) => Some(8.0),
        _ => None,
    }
}

/// Where a point `at` of dimension `from` is in dimension `to` (x and z
/// scaled, y kept), if it has a place there.
pub fn convert(at: [f64; 3], from: &str, to: &str) -> Option<[f64; 3]> {
    let k = dimension_scale(from, to)?;
    Some([at[0] * k, at[1], at[2] * k])
}


// ---- Projection ------------------------------------------------------------

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Where the player looks: yaw 0 faces south (+Z), 90 west; pitch 90 down.
pub fn forward(yaw: f32, pitch: f32) -> [f64; 3] {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}

/// The camera's forward, right and up. Right comes from the yaw alone, so
/// looking straight up or down still has one.
fn basis(yaw: f32, pitch: f32) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let f = forward(yaw, pitch);
    let y = f64::from(yaw).to_radians();
    let r = [-y.cos(), 0.0, -y.sin()];
    (f, r, cross(r, f))
}

/// `point` on a `size` screen, as the world's perspective projection draws
/// it (vertical `fov`, the screen's aspect); None behind the camera.
pub fn project(
    camera: [f64; 3],
    view: ([f64; 3], [f64; 3], [f64; 3]),
    fov: f32,
    point: [f64; 3],
    size: [f32; 2],
) -> Option<[f32; 2]> {
    let (f, r, u) = view;
    let d = sub(point, camera);
    let z = dot(d, f);
    if z < 0.05 || size[1] <= 0.0 {
        return None;
    }
    let t = (f64::from(fov).to_radians() / 2.0).tan();
    let aspect = f64::from(size[0]) / f64::from(size[1]);
    let nx = dot(d, r) / (z * t * aspect);
    let ny = dot(d, u) / (z * t);
    Some([
        ((nx + 1.0) / 2.0 * f64::from(size[0])) as f32,
        ((1.0 - ny) / 2.0 * f64::from(size[1])) as f32,
    ])
}

// ---- Drawing helpers -------------------------------------------------------

struct Out<'a> {
    draws: Vec<Draw>,
    widgets: Vec<Widget>,
    mouse: [f32; 2],
    measure: &'a dyn Fn(&str) -> f32,
}

fn inside(rect: [f32; 4], at: [f32; 2]) -> bool {
    at[0] >= rect[0] && at[0] < rect[0] + rect[2] && at[1] >= rect[1] && at[1] < rect[1] + rect[3]
}

impl Out<'_> {
    fn fill(&mut self, rect: [f32; 4], argb: u32) {
        self.draws.push(Draw::Fill(rect, argb));
    }

    fn text(&mut self, text: &str, at: [f32; 2], rgb: u32) {
        self.draws.push(Draw::Text(text.to_owned(), at, rgb, true));
    }

    fn width(&self, text: &str) -> f32 {
        (self.measure)(text)
    }

    /// A 1 pixel frame round `rect`, filled with `inner`.
    fn boxed(&mut self, rect: [f32; 4], edge: u32, inner: u32) {
        let [x, y, w, h] = rect;
        self.fill(rect, edge);
        self.fill([x + 1.0, y + 1.0, w - 2.0, h - 2.0], inner);
    }

    /// `text` cut to `max` pixels with an ellipsis.
    fn fit(&self, text: &str, max: f32) -> String {
        if self.width(text) <= max {
            return text.to_owned();
        }
        let mut cut: String = text.to_owned();
        while !cut.is_empty() && self.width(&format!("{cut}...")) > max {
            cut.pop();
        }
        format!("{cut}...")
    }

    fn button(&mut self, rect: [f32; 4], label: &str, action: Action, on: bool) {
        let hover = inside(rect, self.mouse);
        let sprite = if hover { "gui/sprites/widget/button_highlighted" } else { "gui/sprites/widget/button" };
        let tint = if on { 0xFFA0_FFA0 } else { 0xFFFF_FFFF };
        self.draws.push(Draw::Nine(sprite, rect, tint));
        let w = self.width(label);
        let at = [(rect[0] + (rect[2] - w) / 2.0).round(), (rect[1] + (rect[3] - 8.0) / 2.0).round()];
        self.text(label, at, INK);
        self.widgets.push(Widget { rect, action });
    }

    /// A button as wide as its label.
    fn button_auto(&mut self, x: f32, y: f32, label: &str, action: Action, on: bool) -> f32 {
        let w = self.width(label) + 8.0;
        self.button([x, y, w, 12.0], label, action, on);
        w
    }
}

/// `2`, `0.5`, `1.3`: a number with at most one decimal.
fn trim_float(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
}

/// A compass rose centred on `at`: two crossed diagonal lines and N E S W.
fn compass_rose(out: &mut Out<'_>, at: [f32; 2], r: f32) {
    let line = |out: &mut Out<'_>, a: [f32; 2], b: [f32; 2], w: f32, argb: u32| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (nx, ny) = (-dy / len * w / 2.0, dx / len * w / 2.0);
        out.draws.push(Draw::Quad(
            [[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]],
            argb,
        ));
    };
    let d = r * 0.7;
    for (a, b) in [([-d, -d], [d, d]), ([d, -d], [-d, d])] {
        let pa = [at[0] + a[0], at[1] + a[1]];
        let pb = [at[0] + b[0], at[1] + b[1]];
        line(out, pa, pb, 2.2, 0xFF10_1010);
        line(out, pa, pb, 1.0, 0xFFFF_FFFF);
    }
    for (letter, [dx, dy]) in [("N", [0.0, -1.0]), ("E", [1.0, 0.0]), ("S", [0.0, 1.0]), ("W", [-1.0, 0.0])] {
        let w = out.width(letter);
        let p = [(at[0] + dx * r - w / 2.0 + 0.5).round(), (at[1] + dy * r - 3.5).round()];
        outlined(&mut out.draws, letter, p, INK, 1.0);
    }
}

/// A ring round `mid`, `radius` to its middle and `width` thick.
fn ring(draws: &mut Vec<Draw>, mid: [f32; 2], radius: f32, width: f32, argb: u32) {
    let n = 48;
    let (r0, r1) = (radius - width / 2.0, radius + width / 2.0);
    let at = |i: usize, r: f32| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        [mid[0] + a.cos() * r, mid[1] + a.sin() * r]
    };
    for i in 0..n {
        draws.push(Draw::Quad([at(i, r0), at(i, r1), at(i + 1, r1), at(i + 1, r0)], argb));
    }
}

/// The letter a waypoint shows as: the first of its name.
pub fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// Black or white, whichever reads better on `rgb`.
fn ink_on(rgb: u32) -> u32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    let light = u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114;
    if light > 140_000 { 0x10_1010 } else { 0xFF_FFFF }
}

/// A waypoint on a map: its initial on a square of its colour, ringed in
/// black (white when `hot`), `half` pixels from the middle to the side, its
/// opacity `alpha` of 255.
fn letter_marker(draws: &mut Vec<Draw>, at: [f32; 2], w: &Waypoint, half: f32, hot: bool, alpha: u32) {
    let [x, y] = [at[0].round(), at[1].round()];
    let ring = if hot { 0xFF_FFFF } else { 0 };
    let color = if w.hidden { 0x77_7777 } else { w.color & RGB };
    let a = alpha << 24;
    draws.push(Draw::Fill([x - half - 1.0, y - half - 1.0, half * 2.0 + 2.0, half * 2.0 + 2.0], a | ring));
    draws.push(Draw::Fill([x - half, y - half, half * 2.0, half * 2.0], a | color));
    if half >= 3.5 {
        let scale = half / 4.5;
        draws.push(Draw::Label(initial(&w.name), [x - 2.2 * scale, y - 3.5 * scale], a | ink_on(color), scale, false));
    }
}

// ---- The mod's behaviour ---------------------------------------------------

impl State {
    /// Back to nothing open (a screen closed by the game, or refused).
    pub fn reset(&mut self) {
        self.mode = Mode::Closed;
        self.confirm_delete = None;
        self.skip_char = None;
        self.map.dragging = false;
    }

    /// Switches to world `key`'s waypoints, read from `dir`, when it changed.
    pub fn load_world(&mut self, key: Option<String>, dir: PathBuf) {
        if self.world == key {
            return;
        }
        self.set = DEFAULT_SET.to_owned();
        self.selected = None;
        self.waypoints = key
            .as_deref()
            .and_then(|k| std::fs::read_to_string(dir.join(world_file(k))).ok())
            .map(|t| decode(&t))
            .unwrap_or_default();
        self.seed = key
            .as_deref()
            .and_then(|k| std::fs::read_to_string(dir.join(seed_file(k))).ok())
            .and_then(|t| t.trim().parse().ok());
        self.mini_key = None;
        self.world = key;
        self.dir = Some(dir);
        self.scroll = 0;
        self.map = MapView {
            bpp: self.map.bpp,
            ..MapView::default()
        };
        self.reset();
    }

    /// The settings, read with `get` the first time.
    pub fn settings_with(&mut self, get: impl Fn(&str) -> Option<String>) -> Settings {
        *self.settings.get_or_insert_with(|| Settings::read(get))
    }

    /// The settings (the defaults before they were read).
    pub fn settings(&self) -> Settings {
        self.settings.unwrap_or_default()
    }

    /// Switches the maps to `dimension` of the current world, kept in `dir`.
    pub fn load_map(&mut self, dimension: Option<String>, dir: Option<PathBuf>) {
        if self.map_dimension == dimension && self.maps.dir == dir {
            return;
        }
        self.maps.open(dir);
        self.map_dimension = dimension;
        self.map_shown = None;
        self.other_maps.open(None);
        self.scans.clear();
        self.shade_cache.clear();
        self.images.clear();
        self.mini_key = None;
        self.map.follow = true;
    }

    /// `/atlas <args>`: the seed is this world's, the rest are settings.
    /// What to say back.
    pub fn command(&mut self, args: &str) -> String {
        let words: Vec<&str> = args.split_whitespace().collect();
        if words.first() == Some(&"seed") {
            let rest = args.trim_start().strip_prefix("seed").unwrap_or("").trim();
            return self.set_seed(rest);
        }
        let mut st = self.settings();
        let (said, changed) = st.command(args);
        if changed {
            self.change_settings(st);
        }
        said
    }

    /// Sets (or with `off`, forgets) this world's seed and keeps it.
    pub fn set_seed(&mut self, text: &str) -> String {
        let (Some(dir), Some(key)) = (self.dir.clone(), self.world.clone()) else {
            return "Atlas: join a world first".into();
        };
        let path = dir.join(seed_file(&key));
        if text.is_empty() {
            return match self.seed {
                Some(seed) => format!("Atlas: this world's seed is {seed}"),
                None => "Atlas: no seed set (/atlas seed <seed>)".into(),
            };
        }
        self.mini_key = None;
        if text == "off" {
            self.seed = None;
            let _ = std::fs::remove_file(path);
            return "Atlas: seed forgotten".into();
        }
        let Some(seed) = parse_seed(text) else {
            return "Atlas: /atlas seed <seed>".into();
        };
        self.seed = Some(seed);
        let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(path, seed.to_string()));
        let hint = if self.settings().slime { "" } else { " (turn on with /atlas slime on)" };
        format!("Atlas: seed {seed} for slime chunks{hint}")
    }

    /// New settings, to be written by the next hook that can.
    pub fn change_settings(&mut self, st: Settings) {
        let old = self.settings();
        // What the scan reads: the loaded chunks are scanned again.
        let scan = |s: &Settings| {
            (s.color_mode, s.flowers, s.plants, s.leaves, s.stained_glass, s.cave_depth, s.transparency)
        };
        if scan(&old) != scan(&st) {
            self.scans.clear();
        }
        if old.shading() != st.shading() {
            self.shade_cache.clear();
        }
        self.settings = Some(st);
        self.settings_changed = true;
        self.mini_key = None;
        // The world map's images are composed again.
        self.images.stamps.clear();
    }

    /// The settings to write, once, when they changed.
    pub fn take_changed_settings(&mut self) -> Option<Settings> {
        std::mem::take(&mut self.settings_changed).then(|| self.settings())
    }

    /// What both maps show from now on.
    pub fn set_cut(&mut self, cut: Cut) {
        if self.cut != cut {
            self.cut = cut;
            self.mini_key = None;
        }
    }

    /// The cut the player's surroundings ask for: Auto changes only once
    /// that has held for the toggle timer, so a tree overhead doesn't flip
    /// the map back and forth.
    pub fn want_cut(&mut self, cut: Cut) {
        let st = self.settings();
        if cut == self.cut || st.mode != MapMode::Auto || st.cave_timer == 0 {
            self.pending_cut = None;
            self.set_cut(cut);
            return;
        }
        match self.pending_cut {
            Some((c, since)) if c == cut => {
                if since.elapsed().as_millis() >= st.cave_timer as u128 {
                    self.pending_cut = None;
                    self.set_cut(cut);
                }
            }
            _ => self.pending_cut = Some((cut, Instant::now())),
        }
    }

    /// Whether loaded chunk `chunk` wants scanning for `cut`: never scanned,
    /// or the world changed since and the last scan is `every` seconds old.
    pub fn wants_scan(&self, cut: Cut, chunk: (i32, i32), revision: u64, every: f64) -> bool {
        match self.scans.get(&(cut, chunk)) {
            None => true,
            Some(&(rev, at)) => rev != revision && at.elapsed().as_secs_f64() >= every,
        }
    }

    /// Chunk `chunk` was scanned for `cut` at world revision `revision`.
    pub fn put_scan(&mut self, cut: Cut, chunk: (i32, i32), revision: u64, tile: Tile) {
        self.scans.insert((cut, chunk), (revision, Instant::now()));
        if self.maps.store(cut).put(chunk.0, chunk.1, tile) && self.shade_cut == Some(cut) {
            // Its columns, and those shaded against them to the south and east.
            for c in [chunk, (chunk.0 + 1, chunk.1), (chunk.0, chunk.1 + 1)] {
                self.shade_cache.remove(&c);
            }
        }
    }

    /// Forgets the scans of chunks more than `radius` chunks from `centre`
    /// (they are rescanned when they load again).
    /// Whether chunk `chunk` of the player's dimension still needs its
    /// new-or-old check (each chunk is checked once, the first time it comes).
    pub fn wants_new_check(&self, chunk: (i32, i32)) -> bool {
        !self.maps.new_chunks.chunks.contains_key(&chunk)
    }

    /// Chunk `chunk` came new (true) or old.
    pub fn put_new_chunk(&mut self, chunk: (i32, i32), new: bool) {
        self.maps.new_chunks.put(chunk, if new { NEW_CHUNK } else { OLD_CHUNK });
    }

    /// Chunk `chunk` of the player's dimension holds portals of `kinds`
    /// (0: none).
    pub fn put_portals(&mut self, chunk: (i32, i32), kinds: u8) {
        self.maps.portals.put(chunk, kinds);
    }

    pub fn forget_far_scans(&mut self, centre: (i32, i32), radius: i32) {
        if self.scans.len() < 4096 {
            return;
        }
        self.scans
            .retain(|(_, (x, z)), _| (x - centre.0).abs() <= radius && (z - centre.1).abs() <= radius);
    }

    /// Writes what changed of the map, at most every `every` seconds.
    pub fn save_now_and_then(&mut self, every: f64) {
        let due = self
            .last_save
            .is_none_or(|t| t.elapsed().as_secs_f64() >= every);
        if due {
            self.maps.save();
            self.last_save = Some(Instant::now());
        }
    }

    /// Lays the minimap's grid over the player at `(x, z)` facing `yaw` and
    /// recomposes its image when what it shows changed: true when it did
    /// (upload `mini_image` then).
    #[cfg(test)]
    pub fn update_minimap(&mut self, x: f64, z: f64, yaw: f32) -> bool {
        self.update_minimap_lit(x, z, yaw, &|_, _, _| 15)
    }

    /// `update_minimap`, with `light` for the block light at a position.
    pub fn update_minimap_lit(&mut self, x: f64, z: f64, yaw: f32, light: &dyn Fn(i32, i32, i32) -> u8) -> bool {
        let seed = self.seed;
        let st = self.settings();
        let bpp = self.mini_zoom() / PIXELS_PER_BLOCK;
        let turning = self.turning();
        let n = self.mini_n;
        let grid = mini_grid(x, z, yaw, turning, bpp, n);
        self.mini_grid = Some(grid);
        let slime = seed.filter(|_| st.slime);
        // The blocks the image can reach.
        let reach = (n as f64 * grid.bpp * std::f64::consts::SQRT_2 / 2.0).ceil() as i32 + 2;
        let (bx, bz) = (x.floor() as i32, z.floor() as i32);
        let cut = self.cut;
        let stamp = self.maps.store(cut).stamp(bx - reach, bz - reach, bx + reach, bz + reach);
        let key = MiniKey {
            origin: grid.origin,
            yaw_bits: if turning { yaw.to_bits() } else { 0 },
            bpp_bits: (bpp).to_bits(),
            n,
            round: st.round,
            rotate: turning,
            cut,
            stamp,
            slime,
            undiscovered: st.undiscovered,
            grid: (st.chunk_grid, st.grid_width),
            light: st.light_overlay.then_some(st.light_level),
            portals: st.portals.then_some(self.maps.portals.revision),
            new_chunks: (st.new_chunks || st.old_chunks).then_some((self.maps.new_chunks.revision, st.new_chunks, st.old_chunks)),
            square: self.view_distance.filter(|_| st.view_square).map(|vd| (bx >> 4, bz >> 4, vd)),
        };
        if self.mini_key == Some(key) && !self.mini_image.is_empty() {
            return false;
        }
        if self.shade_cut != Some(cut) || self.shade_cache.len() > 8192 {
            self.shade_cache.clear();
            self.shade_cut = Some(cut);
        }
        // Xaero's minimap is coloured as its world map, block for block
        // (measured at noon and at midnight).
        let how = self.shading(cut);
        let store = self.maps.store(cut);
        let cache = &mut self.shade_cache;
        // Rows of texels stay in one chunk for a while: remember the last.
        let mut last: Option<((i32, i32), Rc<ShadedChunk>)> = None;
        let mut pixel = |x: i32, z: i32| {
            let chunk = (x >> 4, z >> 4);
            if last.as_ref().is_none_or(|l| l.0 != chunk) {
                let shaded = cache.entry(chunk).or_insert_with(|| shade_chunk(store, chunk, how));
                last = Some((chunk, shaded.clone()));
            }
            let rgba = last.as_ref().map_or([0; 4], |l| l.1[((z & 15) * 16 + (x & 15)) as usize]);
            (rgba != [0; 4]).then_some(rgba)
        };
        compose_minimap(&grid, st.round, st.undiscovered, &mut pixel, &mut self.mini_image);
        if let Some(seed) = slime {
            tint_slime(&grid, st.round, seed, &mut self.mini_image);
        }
        if st.chunk_grid > 0 {
            tint_grid(&grid, st.chunk_grid, st.grid_width, &mut self.mini_image);
        }
        if st.new_chunks || st.old_chunks {
            let shown = |m: u8| (m == NEW_CHUNK && st.new_chunks) || (m == OLD_CHUNK && st.old_chunks);
            tint_chunks(&grid, &mut self.mini_image, |c| self.maps.new_chunks.chunks.get(&c).copied().filter(|m| shown(*m)).map(new_chunk_rgb));
        }
        if st.portals {
            tint_portals(&grid, &self.maps.portals.chunks, &mut self.mini_image);
        }
        if let Some((cx, cz, vd)) = key.square {
            tint_square(&grid, (cx, cz), vd, &mut self.mini_image);
        }
        if st.light_overlay {
            let store = self.maps.store(cut);
            for j in 0..n {
                for i in 0..n {
                    let o = (j * n + i) * 4;
                    let (x, z) = grid.block(i, j);
                    let Some((c, h, ..)) = store.column(x, z) else {
                        continue;
                    };
                    let dark = i32::from(light(x, i32::from(h) + 1, z)) < st.light_level;
                    if c & KIND == LAND && c & WALL == 0 && dark {
                        tint(&mut self.mini_image, o, [230, 30, 30]);
                    }
                }
            }
        }
        self.mini_key = Some(key);
        true
    }

    /// Writes the current world's waypoints.
    pub fn persist(&self) {
        let (Some(dir), Some(key)) = (&self.dir, &self.world) else {
            return;
        };
        let _ = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(dir.join(world_file(key)), encode(&self.waypoints)));
    }

    fn default_name(&self) -> String {
        let mut n = self.waypoints.len() + 1;
        while self.waypoints.iter().any(|w| w.name == format!("Waypoint {n}")) {
            n += 1;
        }
        format!("Waypoint {n}")
    }

    /// The create form, filled in with block `block` of `dimension`.
    pub fn open_create_at(&mut self, block: [i32; 3], dimension: &str, back: Back, skip: Option<char>) {
        let (block, dimension) = self.new_home(block, dimension);
        self.mode = Mode::Edit(Editor {
            index: None,
            name: self.default_name(),
            coords: block.map(|v| v.to_string()),
            dimension,
            color: PALETTE[self.waypoints.len() % PALETTE.len()],
            hidden: false,
            focus: Field::Name,
            back,
            error: None,
            suggested: true,
        });
        self.skip_char = skip;
    }

    /// The create form, filled in with where the player stands.
    pub fn open_create(&mut self, here: &Here, back: Back, skip: Option<char>) {
        let block = here.pos.map(|v| v.floor() as i32);
        self.open_create_at(block, &here.dimension, back, skip);
    }

    pub fn open_list(&mut self, skip: Option<char>) {
        self.mode = Mode::List;
        self.filter_focus = false;
        self.confirm_clear = false;
        self.confirm_delete = None;
        self.skip_char = skip;
    }

    /// The settings screen, at `page`.
    pub fn open_settings(&mut self, page: Page) {
        self.slider = None;
        self.mode = Mode::Settings(SettingsView {
            page,
            sub: 0,
            search: String::new(),
            searching: false,
        });
    }

    /// Back a page: the main page from the others, the game from it.
    fn settings_back(&mut self) {
        let Mode::Settings(v) = &mut self.mode else {
            return;
        };
        if !v.search.is_empty() {
            v.search.clear();
            v.searching = false;
            v.sub = 0;
        } else if v.page == Page::Main {
            self.close();
        } else {
            self.open_settings(Page::Main);
        }
    }

    /// The dragged slider's value at the pointer.
    fn slide(&mut self) {
        let Some((i, rect)) = self.slider else {
            return;
        };
        let Kind::Slider { min, max, .. } = OPTS[i].kind else {
            return;
        };
        let t = ((self.mouse[0] - rect[0] - 4.0) / (rect[2] - 8.0)).clamp(0.0, 1.0);
        let v = min + ((max - min) as f32 * t).round() as i32;
        let mut st = self.settings();
        if st.raw(i) != v {
            st.put(i, v);
            self.change_settings(st);
        }
    }

    /// The world map, on the player when it follows them.
    pub fn open_map(&mut self, here: Option<&Here>, skip: Option<char>) {
        self.mode = Mode::Map;
        self.skip_char = skip;
        self.map.dragging = false;
        if let Some(h) = here
            && self.map.follow
        {
            self.centre_on(h);
        }
    }

    fn open_edit(&mut self, index: usize, back: Back) {
        let Some(w) = self.waypoints.get(index) else {
            return;
        };
        self.mode = Mode::Edit(Editor {
            index: Some(index),
            name: w.name.clone(),
            coords: w.pos.map(|v| v.to_string()),
            dimension: w.dimension.clone(),
            color: w.color,
            hidden: w.hidden,
            focus: Field::Name,
            back,
            error: None,
            suggested: false,
        });
    }

    fn close(&mut self) {
        self.reset();
        self.close_requested = true;
    }

    /// Whether the game should close the screen now (once).
    pub fn take_close(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// The game's screen is (`ours`) or isn't the mod's: forget an open
    /// screen the game closed (Escape, another screen).
    pub fn sync_open(&mut self, ours: bool) {
        self.skip_char = None;
        if !ours && self.mode != Mode::Closed {
            self.reset();
        }
    }

    /// Leaves the editor: back to where it was opened from.
    fn leave_editor(&mut self, back: Back) {
        match back {
            Back::List => self.mode = Mode::List,
            Back::Map => self.mode = Mode::Map,
            Back::Game => self.close(),
        }
    }

    /// Saves the form, or says what is wrong with it.
    fn save(&mut self) {
        let Mode::Edit(editor) = &mut self.mode else {
            return;
        };
        let parsed: Vec<Option<i32>> = editor
            .coords
            .iter()
            .map(|c| c.trim().parse::<i32>().ok())
            .collect();
        let [Some(x), Some(y), Some(z)] = parsed[..] else {
            editor.error = Some("Coordinates must be whole numbers".into());
            return;
        };
        let name = editor.name.trim().to_owned();
        let waypoint = Waypoint {
            name,
            pos: [x, y, z],
            dimension: editor.dimension.clone(),
            color: editor.color,
            hidden: editor.hidden,
            ..Waypoint::default()
        };
        let (index, back) = (editor.index, editor.back);
        let waypoint = if waypoint.name.is_empty() {
            Waypoint {
                name: self.default_name(),
                ..waypoint
            }
        } else {
            waypoint
        };
        match index.filter(|&i| i < self.waypoints.len()) {
            Some(i) => {
                let old = &self.waypoints[i];
                let waypoint = Waypoint { set: old.set.clone(), kind: old.kind, ..waypoint };
                self.waypoints[i] = waypoint;
            }
            None => self.add_waypoint(Waypoint { set: self.set.clone(), ..waypoint }),
        }
        self.persist();
        self.leave_editor(back);
    }

    /// Adds a waypoint at the top of the list, or the bottom if so set.
    fn add_waypoint(&mut self, w: Waypoint) {
        if self.settings().new_to_bottom {
            self.waypoints.push(w);
        } else {
            self.waypoints.insert(0, w);
            self.selected = self.selected.map(|i| i + 1);
        }
    }

    /// Where waypoint `w` shows in `dimension`: its block centre there, or
    /// None. With Overworld Waypoints in Nether, the Overworld's show in the
    /// Nether too, at an eighth of their coordinates.
    fn spot(&self, w: &Waypoint, dimension: &str) -> Option<[f64; 3]> {
        if w.dimension == dimension {
            return Some(centre(w));
        }
        let crossed = self.settings().ow_in_nether && w.dimension == OVERWORLD && dimension == NETHER;
        crossed.then(|| convert(centre(w), OVERWORLD, NETHER)).flatten()
    }

    /// Where a new waypoint at `block` of `dimension` goes: with Overworld
    /// Waypoints in Nether, one made in the Nether is the Overworld's, at
    /// eight times its coordinates.
    fn new_home(&self, block: [i32; 3], dimension: &str) -> ([i32; 3], String) {
        if self.settings().ow_in_nether && dimension == NETHER {
            return ([block[0] * 8, block[1], block[2] * 8], OVERWORLD.to_owned());
        }
        (block, dimension.to_owned())
    }

    /// Whether a waypoint shows on the maps and in the world.
    fn shows(&self, w: &Waypoint) -> bool {
        let st = self.settings();
        !w.hidden && (st.render_all_sets || w.set == self.set) && (w.kind != WpKind::Death || st.deathpoints)
    }

    /// The sets there are, the default one first.
    pub fn sets(&self) -> Vec<String> {
        let mut sets = vec![DEFAULT_SET.to_owned()];
        for w in &self.waypoints {
            if !sets.contains(&w.set) {
                sets.push(w.set.clone());
            }
        }
        if !sets.contains(&self.set) {
            sets.push(self.set.clone());
        }
        sets
    }

    /// A temporary waypoint where the player stands.
    pub fn add_temporary(&mut self, here: &Here) {
        let n = self.waypoints.iter().filter(|w| w.kind == WpKind::Temporary).count() + 1;
        let (pos, dimension) = self.new_home(here.pos.map(|v| v.floor() as i32), &here.dimension);
        self.add_waypoint(Waypoint {
            name: format!("Temp {n}"),
            pos,
            dimension,
            color: 0xFFFF55,
            set: self.set.clone(),
            kind: WpKind::Temporary,
            ..Waypoint::default()
        });
    }

    /// The player is (not) dead, last seen at `here`: a deathpoint where
    /// they died, and old ones go when they reach them.
    pub fn track_death(&mut self, dead: bool, here: &Here) {
        let st = self.settings();
        if dead && !self.was_dead && st.deathpoints {
            if !st.keep_deathpoints {
                self.waypoints.retain(|w| w.kind != WpKind::Death);
                self.selected = None;
            }
            self.add_waypoint(Waypoint {
                name: "Death".to_owned(),
                pos: here.pos.map(|v| v.floor() as i32),
                dimension: here.dimension.clone(),
                color: 0xFF5555,
                set: self.set.clone(),
                kind: WpKind::Death,
                ..Waypoint::default()
            });
            self.persist();
        }
        self.was_dead = dead;
        if !dead && st.delete_reached {
            let near = |w: &Waypoint| {
                w.kind == WpKind::Death && w.dimension == here.dimension && distance(here.pos, centre(w)) < 3.0
            };
            if self.waypoints.iter().any(near) {
                self.waypoints.retain(|w| !near(w));
                self.selected = None;
                self.persist();
            }
        }
    }

    fn delete(&mut self, index: usize) {
        if self.confirm_delete != Some(index) {
            self.confirm_delete = Some(index);
            return;
        }
        self.confirm_delete = None;
        if index < self.waypoints.len() {
            self.waypoints.remove(index);
            self.persist();
        }
        self.selected = match self.selected {
            Some(s) if s == index => None,
            Some(s) if s > index => Some(s - 1),
            other => other,
        };
        // A form open on it from the map goes back there.
        if let Mode::Edit(e) = &self.mode
            && e.index == Some(index)
        {
            let back = e.back;
            self.leave_editor(back);
        }
    }

    /// Does what a click on `action` does.
    pub fn act(&mut self, action: Action, here: Option<&Here>) {
        if !matches!(action, Action::Delete(_)) {
            self.confirm_delete = None;
        }
        match action {
            Action::Opt(i) => {
                let mut st = self.settings();
                st.put(i, OPTS[i].next(st.raw(i)));
                self.change_settings(st);
            }
            Action::Slide(i, rect) => {
                self.slider = Some((i, rect));
                self.slide();
            }
            Action::GoPage(page) => self.open_settings(page),
            Action::SubPage(n) => {
                if let Mode::Settings(v) = &mut self.mode {
                    v.sub = v.sub.saturating_add_signed(n as isize);
                }
            }
            Action::SettingsBack => self.settings_back(),
            Action::FocusSearch => {
                if let Mode::Settings(v) = &mut self.mode {
                    v.searching = true;
                }
            }
            Action::Select(i) => {
                self.selected = (self.selected != Some(i)).then_some(i);
            }
            Action::Teleport(i) => {
                if let Some(w) = self.waypoints.get(i)
                    && self.can_teleport
                {
                    let same = here.is_none_or(|h| h.dimension == w.dimension);
                    if self.settings().cross_dim_tp || same {
                        self.teleport = Some((w.dimension.clone(), w.pos));
                        self.close();
                    }
                }
            }
            Action::CycleFilter => {
                let at = Filter::ALL.iter().position(|f| *f == self.filter).unwrap_or(0);
                self.filter = Filter::ALL[(at + 1) % Filter::ALL.len()];
                self.scroll = 0;
            }
            Action::CycleSet => {
                let sets = self.sets();
                let at = sets.iter().position(|n| *n == self.set).unwrap_or(0);
                self.set = sets[(at + 1) % sets.len()].clone();
                self.selected = None;
                self.confirm_clear = false;
            }
            Action::NewSet => {
                let name = self.list_filter.trim().to_owned();
                self.set = if name.is_empty() { format!("Set {}", self.sets().len() + 1) } else { name };
                self.list_filter.clear();
                self.selected = None;
            }
            Action::ClearSet => {
                if std::mem::take(&mut self.confirm_clear) {
                    let set = self.set.clone();
                    self.waypoints.retain(|w| w.set != set);
                    self.selected = None;
                    self.persist();
                } else {
                    self.confirm_clear = true;
                }
            }
            Action::FocusFilter => self.filter_focus = true,
            Action::CycleMapDimension => {
                let next = next_dimension(&self.shown_dimension());
                self.show_dimension(next, here);
            }
            Action::ToggleAll => {
                let set = self.set.clone();
                let hide = self.waypoints.iter().any(|w| w.set == set && !w.hidden);
                for w in self.waypoints.iter_mut().filter(|w| w.set == set) {
                    w.hidden = hide;
                }
                self.persist();
            }
            Action::AddEdit => match self.selected {
                Some(i) => self.open_edit(i, Back::List),
                None => {
                    if let Some(here) = here {
                        self.open_create(here, Back::List, None);
                    }
                }
            },
            Action::Share(i) => {
                if let Some(w) = self.waypoints.get(i) {
                    let [x, y, z] = w.pos;
                    self.say = Some(format!("[Waypoint] {} at {x} {y} {z} ({})", w.name, dimension_label(&w.dimension)));
                }
            }
            Action::Toggle(i) => {
                if let Some(w) = self.waypoints.get_mut(i) {
                    w.hidden = !w.hidden;
                    self.persist();
                }
            }
            Action::MapWaypoint(i) => self.open_edit(i, Back::Map),
            Action::Delete(i) => self.delete(i),
            Action::Close => self.close(),
            Action::Save => self.save(),
            Action::Cancel => {
                if let Mode::Edit(e) = &self.mode {
                    let back = e.back;
                    self.leave_editor(back);
                }
            }
            Action::Focus(field) => {
                if let Mode::Edit(e) = &mut self.mode {
                    e.focus = field;
                }
            }
            Action::Color(color) => {
                if let Mode::Edit(e) = &mut self.mode {
                    e.color = color;
                }
            }
            Action::CycleDimension => {
                if let Mode::Edit(e) = &mut self.mode {
                    e.dimension = next_dimension(&e.dimension).to_owned();
                }
            }
            Action::ToggleHidden => {
                if let Mode::Edit(e) = &mut self.mode {
                    e.hidden = !e.hidden;
                }
            }
            Action::Here => {
                if let (Mode::Edit(e), Some(here)) = (&mut self.mode, here) {
                    e.coords = here.pos.map(|v| (v.floor() as i32).to_string());
                    e.dimension = here.dimension.clone();
                    e.error = None;
                }
            }
            Action::OpenList => self.open_list(None),
            Action::CycleMapMode => {
                let mut st = self.settings();
                let at = MapMode::ALL.iter().position(|m| *m == st.mode).unwrap_or(0);
                st.mode = MapMode::ALL[(at + 1) % MapMode::ALL.len()];
                self.change_settings(st);
            }
            Action::LayerStep(n) => {
                let mut st = self.settings();
                st.layer_y = (st.layer_y + n).clamp(-2048, 2048);
                self.change_settings(st);
            }
            Action::Centre => {
                self.map.follow = true;
                if let Some(h) = here {
                    self.centre_on(h);
                }
            }
        }
    }

    /// A key pressed over the mod's screen (all of them are the mod's).
    /// `keys` are the player's map, create, list and settings keys.
    pub fn key(&mut self, key: &str, shift: bool, here: Option<&Here>, keys: [&str; 4]) {
        let [map_key, create_key, list_key, settings_key] = keys;
        match &mut self.mode {
            Mode::Closed => {}
            Mode::Settings(v) => match key {
                "key.keyboard.escape" => self.settings_back(),
                "key.keyboard.backspace" if v.searching => {
                    v.search.pop();
                    v.sub = 0;
                }
                k if k == settings_key && !v.searching => self.close(),
                _ => {}
            },
            Mode::List if self.filter_focus => match key {
                "key.keyboard.escape" | "key.keyboard.enter" => self.filter_focus = false,
                "key.keyboard.backspace" => {
                    self.list_filter.pop();
                }
                _ => {}
            },
            Mode::List => match key {
                "key.keyboard.escape" => self.close(),
                "key.keyboard.t" => {
                    if let Some(i) = self.selected {
                        self.act(Action::Teleport(i), here);
                    }
                }
                "key.keyboard.up" => self.scroll = self.scroll.saturating_sub(1),
                "key.keyboard.down" => self.scroll += 1,
                k if k == list_key => self.close(),
                k if k == map_key => self.open_map(here, key_char(k)),
                k if k == create_key => {
                    if let Some(here) = here {
                        self.open_create(here, Back::List, key_char(k));
                    }
                }
                _ => {}
            },
            Mode::Map => {
                let size = self.screen_size;
                let pan = 32.0;
                match key {
                    "key.keyboard.escape" => self.close(),
                    "key.keyboard.up" => self.map.pan([0.0, pan]),
                    "key.keyboard.down" => self.map.pan([0.0, -pan]),
                    "key.keyboard.left" => self.map.pan([pan, 0.0]),
                    "key.keyboard.right" => self.map.pan([-pan, 0.0]),
                    "key.keyboard.equal" | "key.keyboard.keypad.add" => {
                        self.map.zoom(1.0, [size[0] / 2.0, size[1] / 2.0], size);
                    }
                    "key.keyboard.minus" | "key.keyboard.keypad.subtract" => {
                        self.map.zoom(-1.0, [size[0] / 2.0, size[1] / 2.0], size);
                    }
                    "key.keyboard.space" => self.act(Action::Centre, here),
                    k if k == map_key => self.close(),
                    k if k == list_key => self.open_list(key_char(k)),
                    // A new waypoint where the pointer is.
                    k if k == create_key => {
                        let mouse = self.mouse;
                        self.create_at_pointer(mouse, here, key_char(k));
                    }
                    _ => {}
                }
            }
            Mode::Edit(editor) => match key {
                "key.keyboard.escape" => {
                    let back = editor.back;
                    self.leave_editor(back);
                }
                "key.keyboard.enter" | "key.keyboard.keypad.enter" => self.save(),
                "key.keyboard.tab" => editor.focus = editor.focus.step(shift),
                "key.keyboard.up" => editor.focus = editor.focus.step(true),
                "key.keyboard.down" => editor.focus = editor.focus.step(false),
                "key.keyboard.backspace" => {
                    let focus = editor.focus;
                    if focus == Field::Name && editor.suggested {
                        editor.name.clear();
                    } else {
                        editor.field(focus).pop();
                    }
                    editor.suggested = false;
                    editor.error = None;
                }
                "key.keyboard.delete" => {
                    let focus = editor.focus;
                    editor.field(focus).clear();
                    editor.error = None;
                }
                _ => {}
            },
        }
    }

    /// A character typed over the mod's screen.
    pub fn char_typed(&mut self, c: char) {
        if let Some(skip) = self.skip_char.take()
            && skip.eq_ignore_ascii_case(&c)
        {
            return;
        }
        if self.mode == Mode::List {
            if self.filter_focus && !c.is_control() && self.list_filter.chars().count() < NAME_MAX {
                self.list_filter.push(c);
                self.scroll = 0;
            }
            return;
        }
        if let Mode::Settings(v) = &mut self.mode {
            if v.searching && !c.is_control() && v.search.chars().count() < NAME_MAX {
                v.search.push(c);
                v.sub = 0;
            }
            return;
        }
        let Mode::Edit(editor) = &mut self.mode else {
            return;
        };
        let focus = editor.focus;
        if focus == Field::Name && std::mem::take(&mut editor.suggested) {
            editor.name.clear();
        }
        let field = editor.field(focus);
        let ok = match focus {
            Field::Name => field.chars().count() < NAME_MAX && !c.is_control(),
            _ => {
                field.len() < COORD_MAX && (c.is_ascii_digit() || (c == '-' && field.is_empty()))
            }
        };
        if ok {
            field.push(c);
            editor.error = None;
        }
    }

    /// The wheel over the mod's screen: the list scrolls a row a notch, the
    /// map zooms round the pointer.
    pub fn scroll_by(&mut self, steps: f64, mouse: [f32; 2]) {
        if self.mode == Mode::Map {
            let size = self.screen_size;
            self.map.zoom(steps, mouse, size);
            return;
        }
        if matches!(self.mode, Mode::Settings(_)) {
            self.act(Action::SubPage(if steps > 0.0 { -1 } else { 1 }), None);
            return;
        }
        if steps > 0.0 {
            self.scroll = self.scroll.saturating_sub(1);
        } else if steps < 0.0 {
            self.scroll += 1;
        }
    }

    /// A mouse button went down on the mod's screen at `mouse`, over
    /// `widgets` (this frame's).
    pub fn press(&mut self, button: i32, mouse: [f32; 2], widgets: &[Widget], here: Option<&Here>) {
        self.mouse = mouse;
        if let Some(action) = hit(widgets, mouse) {
            if button == 0 {
                self.act(action, here);
            }
            return;
        }
        if self.mode != Mode::Map {
            return;
        }
        match button {
            0 => {
                self.map.dragging = true;
                self.map.dragged = 0.0;
            }
            1 => self.create_at_pointer(mouse, here, None),
            _ => {}
        }
    }

    /// The chat line asked for, once.
    pub fn take_say(&mut self) -> Option<String> {
        self.say.take()
    }

    /// The teleport asked for, once: `/execute in <dimension> run tp @s x y z`.
    pub fn take_teleport(&mut self) -> Option<String> {
        let (dimension, [x, y, z]) = self.teleport.take()?;
        Some(format!("/execute in {dimension} run tp @s {x}.5 {y} {z}.5"))
    }

    /// The dots on the level the maps show: on the surface, entities no
    /// more than a few blocks under the ground the map shows where they
    /// are; in a cut, those inside its band.
    pub fn visible_dots(&mut self, dots: Vec<Dot>, player_y: f64) -> Vec<Dot> {
        let st = self.settings();
        let cut = self.cut;
        let store = self.maps.store(cut);
        dots.into_iter()
            .filter(|d| match d.kind {
                DotKind::Player => st.show_players,
                DotKind::Mob => st.show_mobs,
                DotKind::Item => st.show_items,
                DotKind::Other => st.show_others,
            })
            .filter(|d| st.height_limit == 0 || (d.pos[1] - player_y).abs() <= f64::from(st.height_limit))
            .filter(|d| {
                let y = d.pos[1];
                match cut {
                    Cut::Below(top) => {
                        let top = f64::from(top);
                        y <= top + 2.0 && y >= top - f64::from(st.cave_depth) - 8.0
                    }
                    Cut::Surface => match store.column(d.pos[0].floor() as i32, d.pos[2].floor() as i32) {
                        Some((c, h, ..)) if c & KIND != 0 => y >= f64::from(h) - 3.0,
                        _ => (y - player_y).abs() <= 24.0,
                    },
                }
            })
            .collect()
    }

    /// Whether the world map shows (alone, or under a form opened from it).
    pub fn map_visible(&self) -> bool {
        match &self.mode {
            Mode::Map => true,
            Mode::Edit(e) => e.back == Back::Map,
            _ => false,
        }
    }

    /// The world map's centre on the player, where they would be in the
    /// dimension it shows (the End and the rest: where it already looks).
    fn centre_on(&mut self, h: &Here) {
        if let Some(at) = convert(h.pos, &h.dimension, &self.shown_dimension()) {
            self.map.centre = [at[0], at[2]];
        }
    }

    /// The dimension the world map shows.
    pub fn shown_dimension(&self) -> String {
        self.map_shown.clone().or_else(|| self.map_dimension.clone()).unwrap_or_else(|| OVERWORLD.to_owned())
    }

    /// The cut the world map shows: the player's, or the other
    /// dimension's fullest.
    pub fn map_cut(&self) -> Cut {
        if self.map_shown.is_some() { self.other_cut } else { self.cut }
    }

    /// The maps of the dimension the world map shows.
    fn view_maps(&mut self) -> &mut Maps {
        if self.map_shown.is_some() { &mut self.other_maps } else { &mut self.maps }
    }

    /// The world map shows dimension `to` (the player's own: follows them
    /// again), centred where the player would be there.
    pub fn show_dimension(&mut self, to: &str, here: Option<&Here>) {
        let own = self.map_dimension.clone().unwrap_or_else(|| OVERWORLD.to_owned());
        let from = self.shown_dimension();
        if to == from {
            return;
        }
        if to == own {
            self.map_shown = None;
            self.other_maps.open(None);
            self.map.follow = true;
        } else {
            let dir = match (&self.dir, &self.world) {
                (Some(d), Some(w)) => Some(map_dir(d, w, to)),
                _ => None,
            };
            self.other_maps.open(dir);
            self.other_cut = self.other_maps.fullest_cut();
            self.map_shown = Some(to.to_owned());
            self.map.follow = false;
            let player = here.map_or([self.map.centre[0], 64.0, self.map.centre[1]], |h| h.pos);
            let at = convert(player, &own, to).unwrap_or([0.0; 3]);
            self.map.centre = [at[0], at[2]];
        }
        // Images are keyed by cut and place, not dimension.
        self.images.clear();
    }

    /// A number that changes when what world map image `key` shows does.
    pub fn tile_stamp(&mut self, key: TileKey) -> u64 {
        let [x0, z0, x1, z1] = key.blocks();
        let sky = self.shading(key.cut).sky.to_bits();
        self.view_maps().store(key.cut).stamp(x0, z0, x1, z1) ^ u64::from(sky) << 40
    }

    /// The day time `ticks` now, None where there is no sky (or no clock):
    /// the maps are shaded again when the sky's light changes.
    pub fn set_day_time(&mut self, ticks: Option<i64>) {
        let sky = ticks.map_or(15.0, sky_light);
        if sky != self.sky {
            self.sky = sky;
            self.shade_cache.clear();
            self.mini_key = None;
        }
    }

    /// How the maps shade columns of `cut` now: lit by the sky on the
    /// surface; in caves and cuts by day, always.
    pub fn shading(&self, cut: Cut) -> Shading {
        let sky = if cut == Cut::Surface { self.sky } else { 15.0 };
        Shading { sky, ..self.settings().shading() }
    }

    /// World map image `key`, 128 x 128 RGBA.
    pub fn compose_tile(&mut self, key: TileKey) -> Vec<u8> {
        let mut out = Vec::new();
        let how = self.shading(key.cut);
        compose_map_tile(self.view_maps().store(key.cut), key, how, &mut out);
        out
    }

    /// The block under the world map's GUI pixel `at`: x, the height the
    /// map shows there (a block above its floor), z.
    pub fn map_block(&mut self, at: [f32; 2], here: Option<&Here>) -> [i32; 3] {
        let [x, z] = self.map.world(at, self.screen_size);
        let (x, z) = (x.floor() as i32, z.floor() as i32);
        let cut = self.map_cut();
        let y = self
            .view_maps()
            .store(cut)
            .column(x, z)
            .filter(|c| c.0 & KIND != 0)
            .map(|c| i32::from(c.1) + 1)
            .or_else(|| here.map(|h| h.pos[1].floor() as i32))
            .unwrap_or(64);
        [x, y, z]
    }

    /// The create form for the block under the world map's pointer.
    fn create_at_pointer(&mut self, at: [f32; 2], here: Option<&Here>, skip: Option<char>) {
        if here.is_none() {
            return;
        }
        let dimension = self.shown_dimension();
        let block = self.map_block(at, here);
        self.open_create_at(block, &dimension, Back::Map, skip);
    }

    pub fn release(&mut self, button: i32) {
        if button == 0 {
            self.map.dragging = false;
            self.slider = None;
        }
    }

    /// The pointer moved `delta` with a button held.
    pub fn drag(&mut self, button: i32, delta: [f32; 2]) -> bool {
        if button == 0 && self.slider.is_some() {
            self.mouse[0] += delta[0];
            self.mouse[1] += delta[1];
            self.slide();
            return true;
        }
        if self.mode != Mode::Map || button != 0 || !self.map.dragging {
            return false;
        }
        self.map.dragged += delta[0].abs() + delta[1].abs();
        self.map.pan(delta);
        true
    }

    /// The labels of this dimension's visible waypoints, far ones first so
    /// near ones draw over them.
    /// Distances are from the player's feet (`player`), as the list shows them.
    pub fn labels(
        &self,
        camera: Camera,
        player: [f64; 3],
        dimension: &str,
        size: [f32; 2],
        measure: &dyn Fn(&str) -> f32,
    ) -> Vec<Draw> {
        let st = self.settings();
        if !st.labels {
            return Vec::new();
        }
        let view = basis(camera.yaw, camera.pitch);
        let mut shown: Vec<(f64, [f32; 2], &Waypoint, [f64; 3])> = self
            .waypoints
            .iter()
            .filter(|w| self.shows(w))
            .filter_map(|w| {
                let spot = self.spot(w, dimension)?;
                let d = distance(player, spot);
                if d < f64::from(st.min_render) || (st.max_render > 0 && d > f64::from(st.max_render)) {
                    return None;
                }
                let at = project(camera.pos, view, camera.fov, spot, size)?;
                let on_screen = at[0] >= 0.0 && at[0] <= size[0] && at[1] >= 0.0 && at[1] <= size[1];
                on_screen.then_some((d, at, w, spot))
            })
            .collect();
        shown.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut draws = Vec::new();
        for (d, [x, y], w, spot) in shown {
            // As Xaero's: a see-through square of its colour with its
            // letter, its name small under it, and its distance while it is
            // looked at, placed to the sub-pixel so they glide as the view
            // turns.
            let alpha = st.world_opacity.clamp(0, 100) as u32 * 255 / 100;
            let a = |v: u32| (v * alpha / 255) << 24;
            let half = 4.5 * st.icon_scale;
            let color = w.color & RGB;
            draws.push(Draw::Fill([x - half - 0.5, y - half - 0.5, half * 2.0 + 1.0, half * 2.0 + 1.0], a(0x70)));
            draws.push(Draw::Fill([x - half, y - half, half * 2.0, half * 2.0], a(0xD0) | color));
            let letter = initial(&w.name);
            let ls = st.icon_scale * 0.9;
            let lw = measure(&letter) * ls;
            draws.push(Draw::Label(letter, [x - lw / 2.0 + 0.4, y - 3.6 * ls], a(0xFF) | ink_on(color), ls, false));

            let looking = {
                let to = sub(spot, camera.pos);
                let flat = (to[0] * to[0] + to[2] * to[2]).sqrt().max(1e-6);
                let yaw_to = (-to[0]).atan2(to[2]).to_degrees();
                let pitch_to = (-to[1]).atan2(flat).to_degrees();
                let dy = (yaw_to - f64::from(camera.yaw) + 540.0).rem_euclid(360.0) - 180.0;
                let dp = pitch_to - f64::from(camera.pitch);
                dy.abs() <= f64::from(st.angle_h) && dp.abs() <= f64::from(st.angle_v)
            };
            let show_distance = st.distance_mode == 2 || (st.distance_mode == 1 && looking);
            let mut lines = vec![(w.name.clone(), st.name_scale)];
            if show_distance {
                lines.push((distance_text(d, st.km_at, st.precision), st.distance_scale));
                if !st.name_above {
                    lines.reverse();
                }
            }
            let mut top = y + half + 2.0;
            for (line, scale) in lines {
                let tw = measure(&line) * scale;
                let h = 8.0 * scale + 2.0;
                draws.push(Draw::Fill([x - tw / 2.0 - 2.0, top - 1.0, tw + 4.0, h], a(0x50)));
                draws.push(Draw::Label(line, [x - tw / 2.0, top], a(0xF0) | 0xFF_FFFF, scale, false));
                top += h + 0.5;
            }
        }
        draws
    }

    /// The lines under the minimap.
    fn info_lines(&self, here: &Here, info: &MiniInfo) -> Vec<String> {
        let st = self.settings();
        let mut lines = Vec::new();
        if st.coords {
            let b = here.pos.map(|v| v.floor() as i64);
            lines.push(format!("{}, {}, {}", b[0], b[1], b[2]));
        }
        if st.biome
            && let Some(biome) = &info.biome
        {
            lines.push(biome_label(biome));
        }
        if st.time
            && let Some(t) = info.day_time
        {
            lines.push(time_text(t));
        }
        if st.facing {
            lines.push(facing_text(here.yaw));
        }
        if st.dimension_line {
            lines.push(dimension_label(&here.dimension));
        }
        if st.chunk_line {
            let (cx, cz) = ((here.pos[0].floor() as i64) >> 4, (here.pos[2].floor() as i64) >> 4);
            lines.push(format!("Chunk {cx}, {cz}"));
        }
        if st.light_line
            && let Some((block, sky)) = info.light
        {
            lines.push(format!("Light {block} ({sky} sky)"));
        }
        if st.weather_line
            && let Some((rain, thunder)) = info.weather
        {
            let w = if thunder > 0.5 {
                "Thunder"
            } else if rain > 0.2 {
                "Rain"
            } else {
                "Clear"
            };
            lines.push(w.to_owned());
        }
        if self.cut != Cut::Surface {
            lines.push(self.cut.label());
        }
        lines
    }

    /// The minimap's side on a `gui` sized screen: the share of its height
    /// chosen (never wider than 45% of it, never under 24 pixels).
    pub fn mini_side(&self, gui: [f32; 2]) -> f32 {
        if self.enlarged {
            return (gui[1] * 0.7).min(gui[0] * 0.7).floor();
        }
        let share = self.settings().size as f32 / 100.0;
        (gui[1] * share).min(gui[0] * 0.45).max(24.0).floor()
    }

    /// Sizes the minimap's image for a `gui` screen of `physical` pixels:
    /// a texel a screen pixel, so nothing is stretched.
    pub fn size_image(&mut self, gui: [f32; 2], physical: [f32; 2]) {
        let scale = if gui[1] > 0.0 { physical[1] / gui[1] } else { 1.0 };
        self.gui_scale = scale.max(1.0);
        let n = (self.mini_side(gui) * scale).round() as usize + 1;
        // Even, so the player sits on the grid.
        self.mini_n = (n + n % 2).clamp(32, MINI_MAX);
    }

    /// Whether the image should be uploaded though it didn't change.
    pub fn mini_upload_due(&mut self) -> bool {
        let due = self.mini_uploaded.is_none_or(|t| t.elapsed().as_secs() >= 2);
        if due {
            self.mini_uploaded = Some(Instant::now());
        }
        due
    }

    /// The lines' size in GUI units: the UI Scale is of a font pixel at GUI
    /// scale 2, so they keep their size on screen at any GUI scale.
    fn info_scale(&self) -> f32 {
        (self.settings().info_scale * 2.0 / self.gui_scale).max(0.3)
    }

    /// How far the minimap reaches from the player, in blocks.
    pub fn mini_reach(&self) -> f64 {
        self.mini_n as f64 * f64::from(self.mini_zoom() / PIXELS_PER_BLOCK)
    }

    /// Whether the minimap turns with the player now.
    pub fn turning(&self) -> bool {
        let st = self.settings();
        st.rotate() && (!self.enlarged || st.keep_rotation)
    }

    /// Blocks per minimap pixel now: closer in caves, as set when enlarged.
    pub fn mini_zoom(&self) -> f32 {
        let st = self.settings();
        if self.enlarged && st.enlarged_zoom > 0 {
            return [0.5, 1.0, 2.0, 4.0, 8.0][(st.enlarged_zoom - 1).min(4)];
        }
        if self.cut == Cut::Surface { st.zoom } else { st.zoom / st.cave_zoom.max(1) as f32 }
    }

    /// The enlarge key went down or up.
    pub fn enlarge_key(&mut self, pressed: bool) {
        if self.settings().toggle_enlarged {
            if pressed {
                self.enlarged = !self.enlarged;
            }
        } else {
            self.enlarged = pressed;
        }
        self.mini_key = None;
    }

    /// The zoom keys: `steps` towards more detail or less.
    pub fn zoom_key(&mut self, closer: bool) {
        let mut st = self.settings();
        let i = Opt::index("zoom").unwrap_or(0);
        let Kind::Steps { steps, .. } = OPTS[i].kind else {
            return;
        };
        let now = st.raw(i);
        let at = steps.iter().position(|&v| v >= now).unwrap_or(steps.len() - 1);
        let to = if closer { at.saturating_sub(1) } else { (at + 1).min(steps.len() - 1) };
        st.put(i, steps[to]);
        self.change_settings(st);
    }

    /// How far the frame reaches out of the map on each side.
    fn frame_width(&self) -> f32 {
        match self.settings().style {
            Style::Frame => 2.0,
            Style::Thin => 1.0,
            Style::Bare => 0.0,
        }
    }

    /// The minimap's box, `[w, h]` in GUI pixels: the map, its frame and
    /// the lines under it.
    pub fn minimap_box(&self, gui: [f32; 2], here: &Here, info: &MiniInfo, measure: &dyn Fn(&str) -> f32) -> [f32; 2] {
        let side = self.mini_side(gui) + self.frame_width() * 2.0;
        let lines = self.info_lines(here, info);
        let scale = self.info_scale();
        let widest = lines.iter().map(|l| measure(l) * scale + 4.0).fold(side, f32::max);
        let below = if lines.is_empty() { 0.0 } else { lines.len() as f32 * LINE * scale + 4.0 };
        [widest, side + below]
    }

    /// The minimap with its box's top left at `at` (as `minimap_box` sized
    /// it), as Xaero's minimap looks: the map in a grey bevelled frame (or a
    /// ring), N E S W on the frame, round dots for `dots`, waypoints as
    /// letters, a red arrow for the player and the lines under it.
    pub fn minimap_hud(
        &self,
        gui: [f32; 2],
        at: [f32; 2],
        here: &Here,
        info: &MiniInfo,
        dots: &[Dot],
        measure: &dyn Fn(&str) -> f32,
    ) -> Vec<Draw> {
        let st = self.settings();
        let mut draws = Vec::new();
        let Some(grid) = self.mini_grid.filter(|_| self.mini_ready) else {
            return draws;
        };
        let [bw, _] = self.minimap_box(gui, here, info, measure);
        let side = self.mini_side(gui);
        let fw = self.frame_width();
        let x = (at[0] + (bw - side) / 2.0).round();
        let y = at[1] + fw;
        let half = side / 2.0;
        let mid = [x + half, y + half];

        // The frame, under the map.
        let (band, light, dark) = frame_bands(st.frame_color);
        if st.round {
            match st.style {
                Style::Frame => {
                    ring(&mut draws, mid, half + 1.0, 2.5, 0xFF10_1010);
                    ring(&mut draws, mid, half + 1.0, 1.5, band);
                    ring(&mut draws, mid, half + 0.8, 0.5, light);
                }
                Style::Thin => ring(&mut draws, mid, half + 0.5, 1.0, 0xFF10_1010),
                Style::Bare => {}
            }
        } else {
            let r = |d: f32| [x - d, y - d, side + d * 2.0, side + d * 2.0];
            match st.style {
                Style::Frame => {
                    draws.push(Draw::Fill(r(2.0), 0xFF10_1010));
                    draws.push(Draw::Fill(r(1.5), band));
                    draws.push(Draw::Fill(r(1.0), light));
                    draws.push(Draw::Fill(r(0.5), dark));
                }
                Style::Thin => draws.push(Draw::Fill(r(1.0), 0xFF10_1010)),
                Style::Bare => {}
            }
        }
        // As see-through as set; the light is in the image.
        let alpha = st.opacity.clamp(0, 100) as u32 * 255 / 100;
        draws.push(Draw::ModImage(
            [grid.shift[0], grid.shift[1], grid.shown(), grid.shown()],
            [x, y, side, side],
            alpha << 24 | 0xFF_FFFF,
        ));

        // Where a world point lands, pulled onto the edge when past it.
        let reach = half - 1.0;
        let place = |dx: f64, dz: f64, pull: bool| -> Option<([f32; 2], bool)> {
            let [mut sx, mut sy] = grid.to_screen(dx, dz, side);
            let past = if st.round {
                let r = (sx * sx + sy * sy).sqrt();
                if r > reach {
                    if !pull {
                        return None;
                    }
                    sx *= reach / r;
                    sy *= reach / r;
                }
                r > reach
            } else {
                let past = sx.abs() > reach || sy.abs() > reach;
                if past && !pull {
                    return None;
                }
                sx = sx.clamp(-reach, reach);
                sy = sy.clamp(-reach, reach);
                past
            };
            Some(([mid[0] + sx, mid[1] + sy], past))
        };

        // Dots a little under 1/40 of the map across, as Xaero's, or icons.
        let dot = (side / 100.0).clamp(0.5, 2.0) * st.dot_size as f32 / 2.0;
        let icons = st.icons == 2 || (st.icons == 1 && self.tab_held);
        let icon = (side / 15.0).clamp(4.5, 10.0) * st.icon_size;
        if st.dots {
            let mut sorted: Vec<&Dot> = dots.iter().collect();
            sorted.sort_by_key(|d| d.kind.layer());
            for d in sorted {
                let Some((p, _)) = place(d.pos[0] - here.pos[0], d.pos[2] - here.pos[2], false) else {
                    continue;
                };
                let dy = d.pos[1] - here.pos[1];
                // Darker and fainter the further from the player's height.
                let near = if st.depth_fade { height_fade(dy, st.fade_from as f32) } else { 1.0 };
                let box_at = [p[0] - icon / 2.0, p[1] - icon / 2.0, icon, icon];
                let face = self.faces.get(match &d.icon {
                    Icon::Mob(id) => id.as_str(),
                    _ => "",
                });
                match (&d.icon, face, self.face_slot) {
                    (Icon::Item(item), ..) if icons => draws.push(Draw::Item(item.clone(), [box_at[0], box_at[1]], icon)),
                    (Icon::Skin(skin), ..) if icons => draws.push(Draw::Skin(skin.clone(), box_at)),
                    (Icon::Mob(_), Some(src), Some(slot)) if icons => {
                        let alpha = ((0.3 + 0.7 * near) * 255.0) as u32;
                        // As wide as the face is, as tall as it is.
                        let h = icon * src[3] / src[2];
                        let dst = [box_at[0], p[1] - h / 2.0, icon, h];
                        draws.push(Draw::Image(slot, *src, dst, alpha << 24 | 0xFF_FFFF));
                    }
                    _ => {
                        let r = if d.kind == DotKind::Player { dot * 1.3 } else { dot };
                        // A diamond with a black outline, as Xaero's.
                        let light = 0.35 + 0.65 * near;
                        diamond(&mut draws, p, r + 0.4, fade(0xFF00_0000, 1.0, near));
                        diamond(&mut draws, p, r, fade(d.kind.color(), light, near));
                    }
                }
                let wanted = st.names == 2 || (st.names == 1 && d.kind == DotKind::Player);
                if let Some(name) = d.name.as_ref().filter(|_| wanted) {
                    let tw = measure(name) * 0.5;
                    draws.push(Draw::Label(name.clone(), [p[0] - tw / 2.0, p[1] + 3.0], 0xE0FF_FFFF, 0.5, true));
                }
                if st.entity_height {
                    let text = format!("{:+}", dy.round() as i32);
                    draws.push(Draw::Label(text, [p[0] + 3.0, p[1] - 2.0], 0xE0FF_FFFF, 0.5, true));
                }
            }
        }

        // Waypoints: their letter on their colour, on the edge when far.
        let marks = self
            .waypoints
            .iter()
            .filter(|w| st.on_map && self.shows(w))
            .filter_map(|w| Some((w, self.spot(w, &here.dimension)?)));
        let map_alpha = st.map_opacity.clamp(0, 100) as u32 * 255 / 100;
        for (w, c) in marks {
            if let Some((p, _)) = place(c[0] - here.pos[0], c[2] - here.pos[2], true) {
                letter_marker(&mut draws, p, w, 4.0 * st.map_scale, false, map_alpha);
            }
        }

        // The player: a red arrow as Xaero's, a crosshair or a dot.
        let facing = if self.turning() { 0.0 } else { 180.0 + here.yaw };
        let color = st.arrow_argb();
        let len = (side / 18.0).clamp(4.0, 7.0) * st.arrow_scale;
        match st.main_entity {
            MAIN_ARROW => arrow(&mut draws, mid, facing, len, color),
            1 => {
                // As Xaero's: 2.5% of the map across, arms 0.7% thick, white
                // with a faint edge.
                let l = (side * 0.0105).max(1.0) * st.arrow_scale;
                let w = (side * 0.0036).max(0.35) * st.arrow_scale;
                let alpha = color & 0xFF00_0000;
                let edge = (alpha >> 25) << 24 | 0x30_3040;
                draws.push(Draw::Fill([mid[0] - l - 0.3, mid[1] - w - 0.3, l * 2.0 + 0.6, w * 2.0 + 0.6], edge));
                draws.push(Draw::Fill([mid[0] - w - 0.3, mid[1] - l - 0.3, w * 2.0 + 0.6, l * 2.0 + 0.6], edge));
                let white = alpha | 0xFF_FFFF;
                draws.push(Draw::Fill([mid[0] - l, mid[1] - w, l * 2.0, w * 2.0], white));
                draws.push(Draw::Fill([mid[0] - w, mid[1] - l, w * 2.0, l * 2.0], white));
            }
            _ => {
                disc(&mut draws, mid, len / 3.0 + 0.6, 0xFF10_1010);
                disc(&mut draws, mid, len / 3.0, color);
            }
        }

        // N E S W on the frame.
        if st.compass {
            for (letter, dir) in [("N", [0.0, -1.0]), ("E", [1.0, 0.0]), ("S", [0.0, 1.0]), ("W", [-1.0, 0.0])] {
                let [sx, sy] = grid.to_screen(dir[0], dir[1], 1.0);
                let len = (sx * sx + sy * sy).sqrt().max(1e-6);
                let (ux, uy) = (sx / len, sy / len);
                // A tiny blue tag just inside the edge, as Xaero's.
                let k = (side / 280.0).max(0.2) * st.compass_scale;
                let tag = 4.5 * k;
                let edge = half - tag - 1.0;
                let t = if st.round { edge } else { edge / ux.abs().max(uy.abs()) };
                let at = [mid[0] + ux * t, mid[1] + uy * t];
                draws.push(Draw::Fill([at[0] - tag, at[1] - tag, tag * 2.0, tag * 2.0], 0xB020_38A0));
                let lw = measure(letter) * k;
                let ink = if letter == "N" { NORTH_RGB[st.north_color.min(3)] } else { INK };
                draws.push(Draw::Label(letter.into(), [at[0] - lw / 2.0 + 0.3 * k, at[1] - 3.5 * k], 0xFF00_0000 | ink, k, false));
            }
        }

        // The lines under it.
        let lines = self.info_lines(here, info);
        let top = y + side + fw + 2.0;
        let scale = self.info_scale();
        let row = LINE * scale;
        for (i, line) in lines.iter().enumerate() {
            let lw = measure(line) * scale;
            let tx = match st.info_align {
                1 => x - fw,
                2 => x + side + fw - lw,
                _ => x + half - lw / 2.0,
            };
            let ty = top + i as f32 * row;
            if st.info_bg > 0 {
                let alpha = st.info_bg.clamp(0, 100) as u32 * 255 / 100;
                draws.push(Draw::Fill([tx - 1.0, ty - 1.0, lw + 2.0, row], alpha << 24));
            }
            draws.push(Draw::Label(line.clone(), [tx, ty], 0xFFFF_FFFF, scale, true));
        }
        draws
    }

    /// The open screen's drawing and clickable areas, for a `size` GUI.
    pub fn screen(
        &mut self,
        size: [f32; 2],
        mouse: [f32; 2],
        here: Option<&Here>,
        measure: &dyn Fn(&str) -> f32,
    ) -> (Vec<Draw>, Vec<Widget>) {
        self.screen_size = size;
        self.mouse = mouse;
        let mut out = Out {
            draws: Vec::new(),
            widgets: Vec::new(),
            mouse,
            measure,
        };
        match self.mode.clone() {
            Mode::Closed => {}
            Mode::List => {
                out.fill([0.0, 0.0, size[0], size[1]], SHADE);
                self.draw_list(&mut out, size, here);
            }
            Mode::Edit(editor) => {
                if editor.back == Back::Map {
                    self.draw_map(&mut out, size, here, false);
                }
                out.fill([0.0, 0.0, size[0], size[1]], SHADE);
                draw_editor(&mut out, size, &editor);
            }
            Mode::Map => self.draw_map(&mut out, size, here, true),
            Mode::Settings(view) => {
                out.fill([0.0, 0.0, size[0], size[1]], SHADE);
                self.draw_settings(&mut out, size, &view);
            }
        }
        (out.draws, out.widgets)
    }

    /// The world map, as Xaero's world map looks: what has been seen of
    /// this dimension on black, edge to edge, the scale top left, the
    /// pointer's block top centre, a compass rose top right, the player as
    /// a red arrow, waypoints as letters with their names, and a few
    /// buttons bottom right.
    fn draw_map(&mut self, out: &mut Out<'_>, size: [f32; 2], here: Option<&Here>, live: bool) {
        if let Some(h) = here
            && self.map.follow
        {
            self.centre_on(h);
        }
        out.fill([0.0, 0.0, size[0], size[1]], MAP_VOID);
        for (key, dst) in self.map.tiles(size, self.map_cut()) {
            if let Some(&slot) = self.images.slots.get(&key) {
                let t = MAP_TEXELS as f32;
                out.draws.push(Draw::Image(slot, [0.0, 0.0, t, t], dst, 0xFFFF_FFFF));
            }
        }
        let dimension = self.shown_dimension();
        // The player, where they would be in the dimension shown.
        let player = here.and_then(|h| Some((convert(h.pos, &h.dimension, &dimension)?, h.yaw)));
        self.draw_map_overlays(out, size, player.map(|p| p.0));

        // Waypoints: their letter, and their name over it.
        let mut hot = None;
        for (i, w) in self.waypoints.iter().enumerate() {
            if (w.hidden && !live) || !(w.hidden || self.shows(w)) {
                continue;
            }
            let Some(c) = self.spot(w, &dimension) else {
                continue;
            };
            let at = self.map.screen(c[0], c[2], size);
            if at[0] < -40.0 || at[1] < -20.0 || at[0] > size[0] + 40.0 || at[1] > size[1] + 20.0 {
                continue;
            }
            let rect = [at[0] - 6.0, at[1] - 6.0, 12.0, 12.0];
            let over = live && inside(rect, out.mouse);
            if over {
                hot = Some(i);
            }
            letter_marker(&mut out.draws, at, w, 5.0, over, 255);
            let name_w = out.width(&w.name);
            let top = at[1] - 16.0;
            out.fill([at[0] - name_w / 2.0 - 2.0, top - 1.0, name_w + 4.0, 10.0], LABEL_BG);
            out.text(&w.name, [at[0] - name_w / 2.0, top], if w.hidden { MUTED } else { INK });
            if live {
                out.widgets.push(Widget {
                    rect,
                    action: Action::MapWaypoint(i),
                });
            }
        }

        if let Some((pos, yaw)) = player {
            let at = self.map.screen(pos[0], pos[2], size);
            let st = self.settings();
            arrow(&mut out.draws, at, 180.0 + yaw, 10.0 * st.arrow_scale, st.arrow_argb());
        }

        // Top left: the scale (pixels a block).
        let scale = format!("Scale: {}x", trim_float(1.0 / self.map.bpp));
        out.text(&scale, [4.0, 4.0], INK);

        // Top centre: the block under the pointer.
        if live {
            let [bx, by, bz] = self.map_block(out.mouse, here);
            let mut at = format!("X: {bx}  Y: {by}  Z: {bz}");
            // The same place in the paired dimension, as XaeroPlus's.
            let other = match dimension.as_str() {
                OVERWORLD => Some(("Nether", bx.div_euclid(8), bz.div_euclid(8))),
                NETHER => Some(("Overworld", bx * 8, bz * 8)),
                _ => None,
            };
            if let Some((name, x, z)) = other {
                at += &format!("   {name}: {x}, {z}");
            }
            let w = out.width(&at);
            out.text(&at, [((size[0] - w) / 2.0).round(), 4.0], INK);
        }
        let cut = self.map_cut();
        let mut under = Vec::new();
        if self.map_shown.is_some() {
            under.push(dimension_label(&dimension));
        }
        if cut != Cut::Surface {
            under.push(cut.label());
        }
        if !under.is_empty() {
            let line = under.join(", ");
            let w = out.width(&line);
            out.text(&line, [((size[0] - w) / 2.0).round(), 15.0], MUTED);
        }

        // Top right: the compass rose.
        compass_rose(out, [size[0] - 24.0, 22.0], 14.0);

        // Bottom right: the buttons; bottom left: how to use it.
        let st = self.settings();
        let mut buttons: Vec<(String, Action)> = vec![
            ("Waypoints".into(), Action::OpenList),
            ("Centre".into(), Action::Centre),
        ];
        if st.mode == MapMode::Layer {
            buttons.push(("+".into(), Action::LayerStep(1)));
            buttons.push((format!("Y {}", st.layer_y), Action::LayerStep(0)));
            buttons.push(("-".into(), Action::LayerStep(-1)));
        }
        buttons.push((format!("Mode: {}", st.mode.label()), Action::CycleMapMode));
        buttons.push((dimension_label(&dimension), Action::CycleMapDimension));
        buttons.push(("Settings".into(), Action::GoPage(Page::Main)));
        let mut x = size[0] - 4.0;
        let by = size[1] - 18.0;
        for (label, action) in buttons {
            let w = out.width(&label) + 10.0;
            x -= w;
            if live {
                let on = action == Action::Centre && self.map.follow;
                out.button([x, by, w, 14.0], &label, action, on);
            }
            x -= 3.0;
        }
        let hint = match hot {
            Some(i) => format!("Click to edit {}", self.waypoints[i].name),
            None => "Drag to move, wheel to zoom, B for a waypoint here, Space to centre".into(),
        };
        let hint = out.fit(&hint, x - 8.0);
        out.text(&hint, [4.0, size[1] - 14.0], MUTED);
    }

    /// The world map's portal highlights and the server's view distance
    /// around `player` (where they are in the dimension shown).
    fn draw_map_overlays(&mut self, out: &mut Out<'_>, size: [f32; 2], player: Option<[f64; 3]>) {
        let st = self.settings();
        let chunk_rect = |map: &MapView, x0: i32, z0: i32, x1: i32, z1: i32| {
            let a = map.screen(f64::from(x0) * 16.0, f64::from(z0) * 16.0, size);
            let b = map.screen(f64::from(x1 + 1) * 16.0, f64::from(z1 + 1) * 16.0, size);
            [a[0], a[1], b[0] - a[0], b[1] - a[1]]
        };
        let [x0, z0] = self.map.world([0.0, 0.0], size);
        let [x1, z1] = self.map.world(size, size);
        let (cx0, cz0, cx1, cz1) = ((x0 as i32 >> 4) - 1, (z0 as i32 >> 4) - 1, (x1 as i32 >> 4) + 1, (z1 as i32 >> 4) + 1);
        let map = self.map.clone();
        let on_screen = |&(&(cx, cz), _): &(&(i32, i32), &u8)| cx >= cx0 && cx <= cx1 && cz >= cz0 && cz <= cz1;
        if st.new_chunks || st.old_chunks {
            for (&(cx, cz), &mark) in self.view_maps().new_chunks.chunks.iter().filter(on_screen) {
                if (mark == NEW_CHUNK && st.new_chunks) || (mark == OLD_CHUNK && st.old_chunks) {
                    // As XaeroPlus's: 100 of 255 opaque.
                    out.fill(chunk_rect(&map, cx, cz, cx, cz), 0x6400_0000 | new_chunk_rgb(mark));
                }
            }
        }
        if st.portals {
            for (&(cx, cz), &kinds) in self.view_maps().portals.chunks.iter().filter(on_screen) {
                let mut r = chunk_rect(&map, cx, cz, cx, cz);
                // At least 3 pixels, so they show zoomed far out.
                for k in 0..2 {
                    if r[k + 2] < 3.0 {
                        r[k] -= (3.0 - r[k + 2]) / 2.0;
                        r[k + 2] = 3.0;
                    }
                }
                out.fill(r, 0x9000_0000 | portal_rgb(kinds));
            }
        }
        if st.view_square
            && let (Some(p), Some(vd)) = (player, self.view_distance)
            && self.map_shown.is_none()
        {
            let (cx, cz) = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
            let r = chunk_rect(&self.map, cx - vd, cz - vd, cx + vd, cz + vd);
            let w = 1.0;
            let c = 0xC0FF_FFFF;
            out.fill([r[0], r[1], r[2], w], c);
            out.fill([r[0], r[1] + r[3] - w, r[2], w], c);
            out.fill([r[0], r[1], w, r[3]], c);
            out.fill([r[0] + r[2] - w, r[1], w, r[3]], c);
        }
    }

    /// The settings screen, laid out as Xaero's: the title and page number,
    /// the search box, two columns of buttons and sliders, Prev, Back, Next.
    fn draw_settings(&mut self, out: &mut Out<'_>, size: [f32; 2], view: &SettingsView) {
        let st = self.settings();
        let items = settings_items(view);
        let gap = 8.0;
        let bw = ((size[0] - 24.0 - gap) / 2.0).min(200.0).floor();
        let left = ((size[0] - bw * 2.0 - gap) / 2.0).floor();
        let top = 46.0;
        let bottom = size[1] - 30.0;
        let rows = (((bottom - top) / 24.0).floor() as usize).clamp(1, 6);
        let pages = items.len().div_ceil(rows * 2).max(1);
        let sub = view.sub.min(pages - 1);
        if let Mode::Settings(v) = &mut self.mode {
            v.sub = sub;
        }

        let title = if view.search.trim().is_empty() {
            format!("{} ({}/{pages})", view.page.title(), sub + 1)
        } else {
            format!("Search ({}/{pages})", sub + 1)
        };
        let tw = out.width(&title);
        out.text(&title, [((size[0] - tw) / 2.0).round(), 8.0], INK);

        // The search box, top right.
        let rect = [left + bw + gap, 22.0, bw, 16.0];
        out.draws.push(Draw::Fill(rect, if view.searching { 0xFFFF_FFFF } else { 0xFFA0_A0A0 }));
        out.draws.push(Draw::Fill([rect[0] + 1.0, rect[1] + 1.0, rect[2] - 2.0, rect[3] - 2.0], 0xFF00_0000));
        let shown = if view.searching { format!("{}_", view.search) } else { view.search.clone() };
        if shown.is_empty() {
            out.text("Search...", [rect[0] + 4.0, rect[1] + 4.0], 0x707070);
        } else {
            out.text(&shown, [rect[0] + 4.0, rect[1] + 4.0], INK);
        }
        out.widgets.push(Widget { rect, action: Action::FocusSearch });

        for (n, item) in items.iter().skip(sub * rows * 2).take(rows * 2).enumerate() {
            let x = left + (n % 2) as f32 * (bw + gap);
            let y = top + (n / 2) as f32 * 24.0;
            let rect = [x, y, bw, 20.0];
            match *item {
                Item::Opt(i) => {
                    let o = &OPTS[i];
                    let label = o.text(st.raw(i));
                    if let Kind::Slider { min, max, .. } = o.kind {
                        let t = (st.raw(i) - min) as f32 / (max - min).max(1) as f32;
                        mc_slider(out, rect, &label, t.clamp(0.0, 1.0), Action::Slide(i, rect));
                    } else {
                        mc_button(out, rect, &label, Some(Action::Opt(i)));
                    }
                }
                Item::Link(page) => {
                    let label = page.title().replace("Minimap ", "");
                    mc_button(out, rect, &label, Some(Action::GoPage(page)));
                }
                Item::List => mc_button(out, rect, "Waypoints", Some(Action::OpenList)),
            }
        }

        // Prev, Back, Next.
        let y = size[1] - 26.0;
        let pw = 74.0;
        let prev = (sub > 0).then_some(Action::SubPage(-1));
        let next = (sub + 1 < pages).then_some(Action::SubPage(1));
        mc_button(out, [left, y, pw, 20.0], "<< Prev", prev);
        mc_button(out, [left + bw * 2.0 + gap - pw, y, pw, 20.0], "Next >>", next);
        let back_w = 200.0_f32.min(bw * 2.0 + gap - pw * 2.0 - 8.0);
        mc_button(
            out,
            [((size[0] - back_w) / 2.0).round(), y, back_w, 20.0],
            "Back",
            Some(Action::SettingsBack),
        );
    }

    /// The waypoint screen, as Xaero's: the world and dimension on top, the
    /// set with its buttons, the filter, the list, and a row of buttons for
    /// the picked waypoint.
    fn draw_list(&mut self, out: &mut Out<'_>, size: [f32; 2], here: Option<&Here>) {
        let st = self.settings();
        let w = (size[0] - 16.0).min(400.0).floor();
        let left = ((size[0] - w) / 2.0).floor();
        let half = ((w - 4.0) / 2.0).floor();
        let boxed = |out: &mut Out<'_>, rect: [f32; 4], text: &str| {
            out.boxed(rect, 0xFF20_2030, 0xFF10_1420);
            let t = out.fit(text, rect[2] - 6.0);
            let tw = out.width(&t);
            out.text(&t, [(rect[0] + (rect[2] - tw) / 2.0).round(), rect[1] + 2.0], INK);
        };

        // World and dimension.
        let world = self.world.as_deref().unwrap_or("?");
        let world = world.strip_prefix("world-").or(world.strip_prefix("server-")).unwrap_or(world).to_owned();
        for (x, label) in [(left, "World/Server"), (left + half + 4.0, "Sub-World/Dimension")] {
            let tw = out.width(label);
            out.text(label, [(x + (half - tw) / 2.0).round(), 3.0], MUTED);
        }
        boxed(out, [left, 13.0, half, 12.0], &world);
        let dim_rect = [left + half + 4.0, 13.0, half, 12.0];
        boxed(out, dim_rect, self.filter.label());
        out.widgets.push(Widget { rect: dim_rect, action: Action::CycleFilter });

        // Options, the set, Clear Set.
        let side = 64.0;
        mc_button(out, [left, 28.0, side, 16.0], "Options", Some(Action::GoPage(Page::Waypoints)));
        let clear = if self.confirm_clear { "Sure?" } else { "Clear Set" };
        mc_button(out, [left + w - side, 28.0, side, 16.0], clear, Some(Action::ClearSet));
        let set_w = w - side * 2.0 - 8.0 - 20.0;
        let set_rect = [left + side + 4.0, 30.0, set_w, 12.0];
        boxed(out, set_rect, &self.set.clone());
        out.widgets.push(Widget { rect: set_rect, action: Action::CycleSet });
        mc_button(out, [left + side + 6.0 + set_w, 28.0, 16.0, 16.0], "+", Some(Action::NewSet));
        out.fill([0.0, 47.0, size[0], 1.0], 0x60FF_FFFF);

        // The filter.
        let frect = [left, 51.0, side + 40.0, 13.0];
        out.fill(frect, if self.filter_focus { 0xFFFF_FFFF } else { 0xFFA0_A0A0 });
        out.fill([frect[0] + 1.0, frect[1] + 1.0, frect[2] - 2.0, frect[3] - 2.0], 0xFF00_0000);
        let shown = if self.filter_focus { format!("{}_", self.list_filter) } else { self.list_filter.clone() };
        if shown.is_empty() {
            out.text("Filter...", [frect[0] + 3.0, frect[1] + 3.0], 0x707070);
        } else {
            let t = out.fit(&shown, frect[2] - 6.0);
            out.text(&t, [frect[0] + 3.0, frect[1] + 3.0], INK);
        }
        out.widgets.push(Widget { rect: frect, action: Action::FocusFilter });
        // As XaeroPlus's: the whole set off or on at once.
        let all = if self.waypoints.iter().any(|w| w.set == self.set && !w.hidden) { "Disable All" } else { "Enable All" };
        mc_button(out, [frect[0] + frect[2] + 4.0, 50.0, 62.0, 15.0], all, Some(Action::ToggleAll));

        let wanted = self.list_filter.trim().to_lowercase();
        let rows: Vec<usize> = self
            .waypoints
            .iter()
            .enumerate()
            .filter(|(_, p)| self.filter.matches(&p.dimension) && p.set == self.set)
            .filter(|(_, p)| wanted.is_empty() || p.name.to_lowercase().contains(&wanted))
            .map(|(i, _)| i)
            .collect();
        if self.selected.is_some_and(|i| !rows.contains(&i)) {
            self.selected = None;
        }
        let top = 67.0;
        let bottom = size[1] - 50.0;
        let fits = (((bottom - top) / ROW).floor() as usize).max(1);
        self.scroll = self.scroll.min(rows.len().saturating_sub(fits));

        if rows.is_empty() {
            let msg = "No waypoints here. Press B in the game or on the world map.";
            let msg = out.fit(msg, w - 8.0);
            out.text(&msg, [left + 4.0, top + 6.0], MUTED);
        }
        for (slot, &i) in rows.iter().skip(self.scroll).take(fits).enumerate() {
            let wp = &self.waypoints[i];
            let ry = top + slot as f32 * ROW;
            let rect = [left, ry, w, ROW - 2.0];
            if self.selected == Some(i) {
                out.boxed(rect, FIELD_FOCUS, 0x6040_4060);
            } else {
                out.fill(rect, if inside(rect, out.mouse) { 0x5040_4050 } else { ROW_BG });
            }
            let mut marks = Vec::new();
            letter_marker(&mut marks, [left + 11.0, ry + 11.0], wp, 5.0, false, 255);
            out.draws.extend(marks);
            let dim = dimension_label(&wp.dimension);
            let dist = match here {
                _ if !st.list_distance => String::new(),
                Some(h) if h.dimension == wp.dimension => {
                    distance_text(distance(h.pos, centre(wp)), st.km_at, st.precision)
                }
                _ => "other dimension".into(),
            };
            let (dim_w, dist_w) = (out.width(&dim), out.width(&dist));
            let info_right = left + w - 4.0;
            out.text(&dim, [info_right - dim_w, ry + 3.0], dimension_color(&wp.dimension));
            out.text(&dist, [info_right - dist_w, ry + 3.0 + LINE], MUTED);
            let text_x = left + 21.0;
            let room = info_right - dim_w.max(dist_w) - 6.0 - text_x;
            let tag = match wp.kind {
                WpKind::Normal => "",
                WpKind::Death => " (deathpoint)",
                WpKind::Temporary => " (temporary)",
            };
            let name = out.fit(&format!("{}{tag}", wp.name), room);
            out.text(&name, [text_x, ry + 3.0], if wp.hidden { MUTED } else { INK });
            let coords = if st.hide_coords {
                String::new()
            } else {
                format!("{} {} {}{}", wp.pos[0], wp.pos[1], wp.pos[2], if wp.hidden { "  (disabled)" } else { "" })
            };
            let coords = out.fit(&coords, room);
            out.text(&coords, [text_x, ry + 3.0 + LINE], MUTED);
            out.widgets.push(Widget { rect, action: Action::Select(i) });
        }
        let more = rows.len().saturating_sub(self.scroll + fits);
        if self.scroll > 0 || more > 0 {
            let hint = format!("{} above, {} below", self.scroll, more);
            let tw = out.width(&hint);
            out.text(&hint, [left + w - tw, 54.0], MUTED);
        }

        // Add/Edit, Teleport (T), Share, Enable, Delete; Done.
        out.fill([0.0, size[1] - 48.0, size[0], 1.0], 0x60FF_FFFF);
        let picked = self.selected.and_then(|i| self.waypoints.get(i).map(|p| (i, p.hidden)));
        let act = |a: fn(usize) -> Action| picked.map(|p| a(p.0));
        let enable = if picked.is_some_and(|p| p.1) { "Enable" } else { "Disable" };
        let del = if picked.is_some_and(|p| self.confirm_delete == Some(p.0)) { "Sure?" } else { "Delete" };
        let tp = if self.can_teleport { act(Action::Teleport) } else { None };
        let buttons: [(&str, Option<Action>); 5] = [
            ("Add/Edit", Some(Action::AddEdit)),
            ("Teleport (T)", tp),
            ("Share", act(Action::Share)),
            (enable, act(Action::Toggle)),
            (del, act(Action::Delete)),
        ];
        let gap = 3.0;
        let bw = ((w - gap * 4.0) / 5.0).floor();
        for (k, (label, action)) in buttons.into_iter().enumerate() {
            mc_button(out, [left + k as f32 * (bw + gap), size[1] - 45.0, bw, 18.0], label, action);
        }
        let done_w = 150.0_f32.min(w);
        mc_button(out, [((size[0] - done_w) / 2.0).round(), size[1] - 24.0, done_w, 20.0], "Done", Some(Action::Close));
    }
}

/// A vanilla button: inert when `action` is None.
fn mc_button(out: &mut Out<'_>, rect: [f32; 4], label: &str, action: Option<Action>) {
    let active = action.is_some();
    let hover = active && inside(rect, out.mouse);
    let sprite = if !active {
        "gui/sprites/widget/button_disabled"
    } else if hover {
        "gui/sprites/widget/button_highlighted"
    } else {
        "gui/sprites/widget/button"
    };
    out.draws.push(Draw::Nine(sprite, rect, 0xFFFF_FFFF));
    let label = out.fit(label, rect[2] - 6.0);
    let tw = out.width(&label);
    let ink = if active { INK } else { 0xA0A0A0 };
    out.text(&label, [(rect[0] + (rect[2] - tw) / 2.0).round(), (rect[1] + (rect[3] - 8.0) / 2.0).round()], ink);
    if let Some(action) = action {
        out.widgets.push(Widget { rect, action });
    }
}

/// A vanilla slider, its handle `t` of the way along.
fn mc_slider(out: &mut Out<'_>, rect: [f32; 4], label: &str, t: f32, action: Action) {
    let hover = inside(rect, out.mouse);
    let (track, handle) = if hover {
        ("gui/sprites/widget/slider_highlighted", "gui/sprites/widget/slider_handle_highlighted")
    } else {
        ("gui/sprites/widget/slider", "gui/sprites/widget/slider_handle")
    };
    out.draws.push(Draw::Nine(track, rect, 0xFFFF_FFFF));
    let x = rect[0] + (rect[2] - 8.0) * t;
    out.draws.push(Draw::Nine(handle, [x, rect[1], 8.0, rect[3]], 0xFFFF_FFFF));
    let label = out.fit(label, rect[2] - 6.0);
    let tw = out.width(&label);
    out.text(&label, [(rect[0] + (rect[2] - tw) / 2.0).round(), (rect[1] + (rect[3] - 8.0) / 2.0).round()], INK);
    out.widgets.push(Widget { rect, action });
}

fn draw_editor(out: &mut Out<'_>, size: [f32; 2], e: &Editor) {
    let pw = (size[0] - 16.0).min(250.0);
    let ph = 178.0_f32.min(size[1] - 8.0);
    let px = ((size[0] - pw) / 2.0).round();
    let py = ((size[1] - ph) / 2.0).round().max(4.0);
    out.boxed([px, py, pw, ph], PANEL_EDGE, PANEL);
    let (left, inner) = (px + 6.0, pw - 12.0);

    let title = if e.index.is_some() { "Edit waypoint" } else { "New waypoint" };
    out.text(title, [left, py + 6.0], INK);
    if let Some(i) = e.index {
        let del_w = out.width("Delete") + 8.0;
        out.button([left + inner - del_w, py + 4.0, del_w, 12.0], "Delete", Action::Delete(i), false);
    }

    out.text("Name", [left, py + 20.0], MUTED);
    text_field(out, [left, py + 30.0, inner, 14.0], &e.name, e.focus == Field::Name, Field::Name);

    let third = (inner - 8.0) / 3.0;
    for (i, (field, label)) in [(Field::X, "X"), (Field::Y, "Y"), (Field::Z, "Z")]
        .into_iter()
        .enumerate()
    {
        let x = left + i as f32 * (third + 4.0);
        out.text(label, [x, py + 48.0], MUTED);
        text_field(out, [x, py + 58.0, third, 14.0], &e.coords[i], e.focus == field, field);
    }

    let here_w = out.width("Use my position") + 8.0;
    out.button([left, py + 76.0, here_w, 12.0], "Use my position", Action::Here, false);
    let dim = format!("Dimension: {}", dimension_label(&e.dimension));
    let dim_w = out.width(&dim) + 8.0;
    out.button(
        [left + inner - dim_w, py + 76.0, dim_w, 12.0],
        &dim,
        Action::CycleDimension,
        false,
    );

    out.text("Colour", [left, py + 94.0], MUTED);
    let swatch = ((inner + 2.0) / PALETTE.len() as f32 - 2.0).min(12.0).floor();
    for (i, &color) in PALETTE.iter().enumerate() {
        let x = left + i as f32 * (swatch + 2.0);
        let rect = [x, py + 104.0, swatch, swatch];
        let edge = if color == e.color { FIELD_FOCUS } else { 0xFF00_0000 };
        out.boxed(rect, edge, 0xFF00_0000 | color);
        if color == e.color {
            out.fill([x + 2.0, py + 106.0, swatch - 4.0, swatch - 4.0], 0xFF00_0000 | color);
        }
        out.widgets.push(Widget {
            rect,
            action: Action::Color(color),
        });
    }

    let vis = if e.hidden { "Visible: No" } else { "Visible: Yes" };
    out.button_auto(left, py + 122.0, vis, Action::ToggleHidden, !e.hidden);

    if let Some(error) = &e.error {
        out.text(error, [left, py + 138.0], ERROR);
    } else {
        out.text("Enter save  -  Tab next field  -  Esc cancel", [left, py + 138.0], MUTED);
    }

    let bw = (inner - 4.0) / 2.0;
    out.button([left, py + ph - 20.0, bw, 14.0], "Save", Action::Save, false);
    out.button([left + bw + 4.0, py + ph - 20.0, bw, 14.0], "Cancel", Action::Cancel, false);
}

fn text_field(out: &mut Out<'_>, rect: [f32; 4], text: &str, focused: bool, field: Field) {
    out.boxed(rect, if focused { FIELD_FOCUS } else { FIELD_EDGE }, FIELD);
    let shown = if focused { format!("{text}_") } else { text.to_owned() };
    // Long text shows its end, where the typing is.
    let max = rect[2] - 6.0;
    let mut tail = shown.as_str();
    while out.width(tail) > max && !tail.is_empty() {
        let mut chars = tail.chars();
        chars.next();
        tail = chars.as_str();
    }
    out.text(tail, [rect[0] + 3.0, rect[1] + 3.0], INK);
    out.widgets.push(Widget {
        rect,
        action: Action::Focus(field),
    });
}

/// The widget under `at`, the last drawn winning.
pub fn hit(widgets: &[Widget], at: [f32; 2]) -> Option<Action> {
    widgets
        .iter()
        .rev()
        .find(|w| inside(w.rect, at))
        .map(|w| w.action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 4] = [DEFAULT_MAP, DEFAULT_CREATE, DEFAULT_LIST, DEFAULT_SETTINGS];

    fn here(dim: &str) -> Here {
        Here {
            pos: [10.7, 64.0, -3.2],
            dimension: dim.into(),
            yaw: 0.0,
        }
    }

    fn measure(s: &str) -> f32 {
        s.chars().count() as f32 * 6.0
    }

    fn grass(height: i16) -> Tile {
        let mut t = Tile::blank(-64);
        t.color = [LAND | 0x5D_A040; 256];
        t.height = [height; 256];
        t
    }

    #[test]
    fn storage_round_trips_odd_names() {
        let list = vec![
            Waypoint {
                name: "Base\twith\\tab".into(),
                pos: [-12, 70, 300],
                dimension: NETHER.into(),
                color: 0xFF5555,
                hidden: true,
                ..Waypoint::default()
            },
            Waypoint {
                name: "Ünïcode ⛏".into(),
                pos: [0, -64, 0],
                dimension: OVERWORLD.into(),
                color: 0x00AA00,
                hidden: false,
                ..Waypoint::default()
            },
        ];
        assert_eq!(decode(&encode(&list)), list);
        assert!(decode("garbage\n1\t2").is_empty());
    }

    #[test]
    fn world_files_are_plain_names() {
        assert_eq!(world_file("server-mc.hypixel.net:25565"), "server-mc.hypixel.net_25565.txt");
        assert_eq!(world_file("world-New World"), "world-New_World.txt");
        let d = map_dir(std::path::Path::new("/d"), "world-A B", NETHER);
        assert_eq!(d, PathBuf::from("/d/maps/world-A_B/minecraft_the_nether"));
    }

    #[test]
    fn words() {
        assert_eq!(dimension_label(NETHER), "Nether");
        assert_eq!(dimension_label("mymod:deep_dark"), "deep dark");
        assert_eq!(distance_text(4.44, 10000, 1), "4.4m");
        assert_eq!(distance_text(15300.0, 10000, 1), "15.3km");
        assert_eq!(distance_text(99.0, 10000, 0), "99m");
        assert_eq!(key_char("key.keyboard.n"), Some('n'));
        assert_eq!(key_char("key.keyboard.f1"), None);
        assert_eq!(trim_float(1.0), "1");
        assert_eq!(trim_float(0.25), "0.3");
        assert_eq!(trim_float(4.0), "4");
    }

    #[test]
    fn projection_puts_what_is_ahead_in_the_middle() {
        let size = [400.0, 200.0];
        let view = basis(0.0, 0.0);
        // Facing south: a point straight ahead is in the centre...
        let c = project([0.0; 3], view, 70.0, [0.0, 0.0, 10.0], size).unwrap();
        assert!((c[0] - 200.0).abs() < 0.01 && (c[1] - 100.0).abs() < 0.01);
        // ...east (+X) is on the left, up is up, behind is nothing.
        let e = project([0.0; 3], view, 70.0, [3.0, 2.0, 10.0], size).unwrap();
        assert!(e[0] < 200.0 && e[1] < 100.0);
        assert!(project([0.0; 3], view, 70.0, [0.0, 0.0, -5.0], size).is_none());
        // Looking straight down still has a right vector.
        let down = basis(0.0, 90.0);
        let below = project([0.0; 3], down, 70.0, [0.0, -10.0, 0.0], size).unwrap();
        assert!((below[0] - 200.0).abs() < 0.01 && (below[1] - 100.0).abs() < 0.01);
        // The edge of the vertical field of view is the screen's edge.
        let t = (35f64).to_radians().tan();
        let top = project([0.0; 3], view, 70.0, [0.0, 10.0 * t, 10.0], size).unwrap();
        assert!(top[1].abs() < 0.01);
    }

    #[test]
    fn create_list_edit_hide_delete() {
        let mut s = State::default();
        s.open_create(&here(OVERWORLD), Back::Game, Some('b'));
        // The opening key's character is not typed.
        s.char_typed('b');
        s.char_typed('!');
        // Typing replaces the suggested name.
        let Mode::Edit(e) = &s.mode else { panic!() };
        assert_eq!(e.name, "!");
        assert_eq!(e.coords, ["10".to_string(), "64".into(), "-4".into()]);
        s.key("key.keyboard.backspace", false, None, KEYS);
        for c in "Home".chars() {
            s.char_typed(c);
        }
        s.key("key.keyboard.tab", false, None, KEYS);
        s.char_typed('x'); // not a number: ignored
        s.act(Action::Color(0x5555FF), None);
        s.key("key.keyboard.enter", false, None, KEYS);
        assert_eq!(s.waypoints.len(), 1);
        assert_eq!(s.waypoints[0].name, "Home");
        assert_eq!(s.waypoints[0].color, 0x5555FF);
        assert!(s.take_close());

        s.open_list(None);
        s.act(Action::Toggle(0), None);
        assert!(s.waypoints[0].hidden);
        s.selected = Some(0);
        s.act(Action::AddEdit, None);
        s.act(Action::CycleDimension, None);
        s.act(Action::Save, None);
        assert_eq!(s.waypoints[0].dimension, NETHER);
        assert_eq!(s.mode, Mode::List);

        s.act(Action::Delete(0), None);
        assert_eq!(s.waypoints.len(), 1, "asks first");
        s.act(Action::Delete(0), None);
        assert!(s.waypoints.is_empty());
    }

    #[test]
    fn bad_coordinates_are_refused() {
        let mut s = State::default();
        s.open_create(&here(OVERWORLD), Back::List, None);
        s.act(Action::Focus(Field::Y), None);
        s.key("key.keyboard.delete", false, None, KEYS);
        s.act(Action::Save, None);
        let Mode::Edit(e) = &s.mode else { panic!() };
        assert!(e.error.is_some());
        assert!(s.waypoints.is_empty());
    }

    #[test]
    fn screens_draw_and_clicks_find_their_buttons() {
        let mut s = State::default();
        s.waypoints.push(Waypoint {
            name: "Far away base with a very long name indeed".into(),
            pos: [100, 64, 100],
            dimension: OVERWORLD.into(),
            color: 0xFFAA00,
            hidden: false,
            ..Waypoint::default()
        });
        s.open_list(None);
        let (draws, widgets) = s.screen([480.0, 270.0], [0.0, 0.0], Some(&here(OVERWORLD)), &measure);
        assert!(draws.iter().any(|d| matches!(d, Draw::Text(t, _, _, _) if t == "Overworld")));
        // Nothing picked: the buttons for one are inert.
        assert!(!widgets.iter().any(|w| w.action == Action::Share(0)));
        let row = widgets.iter().find(|w| w.action == Action::Select(0)).unwrap();
        let at = [row.rect[0] + 1.0, row.rect[1] + 1.0];
        s.press(0, at, &widgets, None);
        assert_eq!(s.selected, Some(0));
        let (_, widgets) = s.screen([480.0, 270.0], [0.0, 0.0], Some(&here(OVERWORLD)), &measure);
        let share = widgets.iter().find(|w| w.action == Action::Share(0)).unwrap();
        let at = [share.rect[0] + 1.0, share.rect[1] + 1.0];
        assert_eq!(hit(&widgets, at), Some(Action::Share(0)));
        // Teleport only for operators, and it closes the screen.
        assert!(!widgets.iter().any(|w| w.action == Action::Teleport(0)));
        s.can_teleport = true;
        let (_, widgets) = s.screen([480.0, 270.0], [0.0, 0.0], Some(&here(OVERWORLD)), &measure);
        assert!(widgets.iter().any(|w| w.action == Action::Teleport(0)));
        s.act(Action::Teleport(0), None);
        assert_eq!(
            s.take_teleport().as_deref(),
            Some("/execute in minecraft:overworld run tp @s 100.5 64 100.5")
        );
        assert!(s.take_close());
        s.open_list(None);

        s.selected = Some(0);
        s.act(Action::AddEdit, None);
        let (_, widgets) = s.screen([480.0, 270.0], [0.0, 0.0], None, &measure);
        assert!(widgets.iter().any(|w| w.action == Action::Color(0xFF55FF)));
        assert!(widgets.iter().any(|w| w.action == Action::Save));
    }

    #[test]
    fn labels_show_only_visible_waypoints_of_this_dimension() {
        let mut s = State::default();
        let camera = Camera {
            pos: [0.5, 65.62, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            fov: 70.0,
        };
        let mk = |name: &str, dim: &str, hidden| Waypoint {
            name: name.into(),
            pos: [0, 65, 20],
            dimension: dim.into(),
            color: 0x55FFFF,
            hidden,
            ..Waypoint::default()
        };
        s.waypoints = vec![mk("A", OVERWORLD, false), mk("B", OVERWORLD, true), mk("C", NETHER, false)];
        let draws = s.labels(camera, [0.5, 64.0, 0.5], OVERWORLD, [480.0, 270.0], &measure);
        let texts: Vec<&str> = draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text(t, ..) | Draw::Label(t, ..) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts[..2], ["A", "A"]);
        assert!(texts[2].ends_with('m'), "the distance while looked at: {texts:?}");
        s.settings = Some(Settings {
            labels: false,
            ..Settings::default()
        });
        assert!(s.labels(camera, [0.5, 64.0, 0.5], OVERWORLD, [480.0, 270.0], &measure).is_empty());
    }

    #[test]
    fn packbits_round_trips() {
        let mut data = vec![7u8; 300];
        data.extend(0..=255u8);
        data.extend([1, 1, 2, 2, 2, 3]);
        let packed = pack(&data);
        assert!(packed.len() < data.len());
        assert_eq!(unpack(&packed, data.len()).unwrap(), data);
        assert_eq!(unpack(&pack(&[]), 0).unwrap(), Vec::<u8>::new());
        assert!(unpack(&[5, 1, 2], 6).is_none(), "cut short");
    }

    #[test]
    fn regions_round_trip_and_the_store_keeps_what_it_saw() {
        let dir = std::env::temp_dir().join(format!("atlas-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::default();
        store.open(Some(dir.clone()));
        let mut t = grass(70);
        t.color[5] = WATER | 0x3F_76E4;
        t.depth[5] = 4;
        t.height[17] = -12;
        t.light[17] = 13;
        assert!(store.put(-1, 33, t.clone()));
        assert!(!store.put(-1, 33, t.clone()), "the same tile again is no change");
        assert!(store.has(-1, 33));
        assert!(!store.has(0, 33));
        // Chunk (-1, 33) is blocks x -16..-1, z 528..543.
        assert_eq!(store.column(-16 + 5, 528), Some((WATER | 0x3F_76E4, 70, 4, 0)));
        assert_eq!(store.column(-16 + 1, 529), Some((LAND | 0x5D_A040, -12, 0, 13)));
        assert_eq!(store.column(5, 528), None);
        let before = store.stamp(-16, 528, -1, 543);
        store.put(-1, 33, grass(71));
        assert_ne!(store.stamp(-16, 528, -1, 543), before);
        store.save();
        // Read back by a new store.
        let mut again = Store::default();
        again.open(Some(dir.clone()));
        assert_eq!(again.column(-16 + 3, 530), Some((LAND | 0x5D_A040, 71, 0, 0)));
        assert!(!again.has(-2, 33));
        let _ = std::fs::remove_dir_all(&dir);
        // A region from before block light reads as unlit.
        let mut region = Region::new(0);
        region.chunks[0] = Some(Box::new(t.clone()));
        let mut old = encode_region(&region);
        old[..4].copy_from_slice(MAGIC_2);
        let len = u32::from_le_bytes(old[4..8].try_into().unwrap()) as usize;
        let mut raw = unpack(&old[8..], len).unwrap();
        raw.truncate(raw.len() - 256);
        old.truncate(4);
        old.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        old.extend(pack(&raw));
        let read = decode_region(&old, 0).unwrap();
        let tile = read.chunks[0].as_deref().unwrap();
        assert_eq!((tile.color, tile.height, tile.depth), (t.color, t.height, t.depth));
        assert_eq!(tile.light, [0; 256]);
    }

    #[test]
    fn overworld_waypoints_show_in_the_nether_at_an_eighth() {
        let mut s = State { settings: Some(Settings { ow_in_nether: true, ..Settings::default() }), ..State::default() };
        let base = Waypoint { name: "Base".into(), pos: [800, 70, -160], ..Waypoint::default() };
        s.waypoints.push(base.clone());
        assert_eq!(s.spot(&base, NETHER), Some([100.0625, 70.5, -19.9375]));
        assert_eq!(s.spot(&base, OVERWORLD), Some([800.5, 70.5, -159.5]));
        assert_eq!(s.spot(&base, END), None);
        // It is drawn in the Nether's world, where it stands there.
        let camera = Camera { pos: [100.5, 71.6, -10.0], yaw: 180.0, pitch: 0.0, fov: 70.0 };
        let labels = s.labels(camera, [100.5, 70.0, -10.0], NETHER, [480.0, 270.0], &measure);
        assert!(!labels.is_empty());
        // One made in the Nether is the Overworld's, eight times as far.
        let h = Here { pos: [10.5, 64.0, -3.5], dimension: NETHER.into(), yaw: 0.0 };
        s.open_create(&h, Back::Game, None);
        let Mode::Edit(e) = &s.mode else { panic!() };
        assert_eq!((e.coords.clone(), e.dimension.as_str()), (["80", "64", "-32"].map(String::from), OVERWORLD));
        // Off, the Nether shows only its own.
        s.settings = Some(Settings::default());
        assert_eq!(s.spot(&base, NETHER), None);
        assert!(s.labels(camera, [100.5, 70.0, -10.0], NETHER, [480.0, 270.0], &measure).is_empty());
    }

    #[test]
    fn disable_all_turns_the_set_off_then_on() {
        let mut s = State::default();
        for (name, set) in [("a", DEFAULT_SET), ("b", DEFAULT_SET), ("c", "other")] {
            s.waypoints.push(Waypoint { name: name.into(), set: set.into(), ..Waypoint::default() });
        }
        s.waypoints[1].hidden = true;
        s.act(Action::ToggleAll, None);
        assert_eq!(s.waypoints.iter().map(|w| w.hidden).collect::<Vec<_>>(), [true, true, false]);
        s.act(Action::ToggleAll, None);
        assert_eq!(s.waypoints.iter().map(|w| w.hidden).collect::<Vec<_>>(), [false, false, false]);
        // The list has the button.
        s.open_list(None);
        let (_, w) = s.screen([480.0, 270.0], [0.0, 0.0], None, &measure);
        assert!(w.iter().any(|x| x.action == Action::ToggleAll));
    }

    #[test]
    fn portals_are_kept_per_dimension_and_highlighted() {
        let dir = std::env::temp_dir().join(format!("atlas-test-portals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut maps = Maps::default();
        maps.open(Some(dir.clone()));
        assert!(maps.portals.put((3, -2), NETHER_PORTAL));
        assert!(!maps.portals.put((3, -2), NETHER_PORTAL), "the same again is no change");
        assert!(maps.portals.put((-40, 7), END_GATEWAY | END_PORTAL));
        assert!(maps.portals.put((9, 9), NETHER_PORTAL));
        // A portal broken: the chunk is scanned again with none.
        assert!(maps.portals.put((9, 9), 0));
        assert!(maps.save());
        let mut again = Maps::default();
        again.open(Some(dir.clone()));
        assert_eq!(again.portals.chunks, HashMap::from([((3, -2), NETHER_PORTAL), ((-40, 7), END_GATEWAY | END_PORTAL)]));
        let _ = std::fs::remove_dir_all(&dir);
        // Bad lines are skipped.
        assert_eq!(ChunkMarks::decode("1 2 1\nx 3 1\n4 5 0\n").chunks.len(), 1);

        // The minimap tints a portal's chunk and redraws when one is found.
        let mut s = State { settings: Some(Settings { portals: true, ..Settings::default() }), ..State::default() };
        s.maps.store(Cut::Surface).put(0, 0, grass(64));
        s.update_minimap(8.0, 8.0, 0.0);
        let before = s.mini_image.clone();
        s.put_portals((0, 0), NETHER_PORTAL);
        assert!(s.update_minimap(8.0, 8.0, 0.0), "a new portal redraws it");
        assert_ne!(s.mini_image, before);
        // The world map fills its chunk.
        let h = here(OVERWORLD);
        s.open_map(Some(&h), None);
        let (draws, _) = s.screen([400.0, 200.0], [0.0, 0.0], Some(&h), &measure);
        assert!(draws.iter().any(|d| matches!(d, Draw::Fill(_, c) if c & RGB == portal_rgb(NETHER_PORTAL))));
    }

    /// `indices` packed `bits` to a field, as a section is sent.
    fn pack_indices(indices: &[usize], bits: u8) -> Vec<u64> {
        let per = 64 / usize::from(bits);
        let mut words = vec![0u64; 4096_usize.div_ceil(per)];
        for (i, &v) in indices.iter().enumerate() {
            words[i / per] |= (v as u64) << ((i % per) * usize::from(bits));
        }
        words
    }

    #[test]
    fn a_palette_the_server_never_saved_is_fresh() {
        // Linear: a saved section's entries are first met in palette order.
        let mut idx = vec![0usize; 4096];
        idx[100] = 1;
        idx[200] = 2;
        let saved = pack_indices(&idx, 4);
        assert!(!fresh_palette(4, &[0, 1, 9], &saved));
        // Out of order: generated, never saved.
        idx[100] = 2;
        idx[200] = 1;
        assert!(fresh_palette(4, &[0, 1, 9], &pack_indices(&idx, 4)));
        // Hash map: an entry nothing uses means it was never compacted.
        let idx: Vec<usize> = (0..4096).map(|i| i % 20).collect();
        let palette: Vec<u32> = (0..20).collect();
        assert!(!fresh_palette(5, &palette, &pack_indices(&idx, 5)));
        let idx: Vec<usize> = (0..4096).map(|i| i % 19).collect();
        assert!(fresh_palette(5, &palette, &pack_indices(&idx, 5)));
        // One block kind, or the global palette: nothing to tell.
        assert!(!fresh_palette(0, &[1], &[]));
        assert!(!fresh_palette(15, &[], &[0; 1024]));
        // A chunk needs three fresh sections: one may be a tick's change.
        let fresh = pack_indices(&{ let mut v = vec![0; 4096]; v[5] = 2; v[6] = 1; v }, 4);
        let pal = [0u32, 1, 2];
        let sec = (4u8, &pal[..], &fresh[..]);
        let plain = (4u8, &pal[..], &saved[..]);
        assert!(!is_new_chunk([sec, sec, plain, plain]));
        assert!(is_new_chunk([sec, plain, sec, sec]));
    }

    #[test]
    fn new_and_old_chunks_are_kept_and_drawn() {
        let dir = std::env::temp_dir().join(format!("atlas-test-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = State { settings: Some(Settings { new_chunks: true, ..Settings::default() }), ..State::default() };
        s.load_map(Some(OVERWORLD.into()), Some(dir.clone()));
        assert!(s.wants_new_check((0, 0)));
        s.put_new_chunk((0, 0), true);
        s.put_new_chunk((1, 0), false);
        assert!(!s.wants_new_check((0, 0)), "each chunk is checked once");
        assert!(s.maps.save());
        let mut again = Maps::default();
        again.open(Some(dir.clone()));
        assert_eq!(again.new_chunks.chunks, HashMap::from([((0, 0), NEW_CHUNK), ((1, 0), OLD_CHUNK)]));
        let _ = std::fs::remove_dir_all(&dir);
        // Red on the world map for the new; the old only when asked for.
        let h = here(OVERWORLD);
        s.open_map(Some(&h), None);
        let fills = |s: &mut State, rgb: u32| {
            let (draws, _) = s.screen([400.0, 200.0], [0.0, 0.0], Some(&h), &measure);
            draws.iter().filter(|d| matches!(d, Draw::Fill(_, c) if c & 0xFF00_0000 == 0x6400_0000 && c & RGB == rgb)).count()
        };
        assert_eq!(fills(&mut s, 0xFF_0000), 1);
        assert_eq!(fills(&mut s, 0x00_FF00), 0);
        s.settings = Some(Settings { new_chunks: true, old_chunks: true, ..Settings::default() });
        assert_eq!(fills(&mut s, 0x00_FF00), 1);
    }

    #[test]
    fn the_view_distance_square_edges_its_chunks() {
        let grid = mini_grid(8.0, 8.0, 0.0, false, 1.0, 120);
        let mut image = vec![100u8; 120 * 120 * 4];
        tint_square(&grid, (0, 0), 1, &mut image);
        let at = |x: i32, z: i32| {
            let (i, j) = (0..120).flat_map(|j| (0..120).map(move |i| (i, j))).find(|&(i, j)| grid.block(i, j) == (x, z)).unwrap();
            image[(j * 120 + i) * 4]
        };
        // The square is chunks -1..=1: blocks -16..=31.
        assert_eq!(at(-16, 0), 255);
        assert_eq!(at(31, 5), 255);
        assert_eq!(at(5, -16), 255);
        assert_eq!(at(0, 0), 100);
        assert_eq!(at(-17, 0), 100);
    }

    #[test]
    fn the_world_map_shows_another_dimensions_map() {
        let data = std::env::temp_dir().join(format!("atlas-test-dims-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        // A Nether map saved earlier: a cave cut with a tile at chunk (1, 1).
        let mut nether = Maps::default();
        nether.open(Some(map_dir(&data, "world-x", NETHER)));
        nether.store(Cut::Below(40)).put(1, 1, grass(30));
        nether.portals.put((1, 1), NETHER_PORTAL);
        assert!(nether.save());

        let mut s = State::default();
        s.load_world(Some("world-x".into()), data.clone());
        s.load_map(Some(OVERWORLD.into()), Some(map_dir(&data, "world-x", OVERWORLD)));
        s.maps.store(Cut::Surface).put(0, 0, grass(70));
        let h = Here { pos: [160.0, 70.0, 80.0], dimension: OVERWORLD.into(), yaw: 0.0 };
        s.open_map(Some(&h), None);
        let size = [400.0, 200.0];
        let (_, w) = s.screen(size, [0.0, 0.0], Some(&h), &measure);
        assert!(w.iter().any(|x| x.action == Action::CycleMapDimension));
        s.act(Action::CycleMapDimension, Some(&h));
        assert_eq!(s.shown_dimension(), NETHER);
        assert_eq!(s.map_cut(), Cut::Below(40), "the Nether's fullest cut");
        assert_eq!(s.map.centre, [20.0, 10.0], "where the player would be there");
        assert!(!s.map.follow);
        // Its tiles come from the Nether's map, not the Overworld's.
        let tile = s.compose_tile(TileKey { cut: Cut::Below(40), level: 0, x: 0, z: 0 });
        let o = ((20 * MAP_TEXELS + 20) * 4) as usize;
        assert_ne!(tile[o + 3], 0);
        assert_eq!(tile[(4 * MAP_TEXELS * 4 + 4 * 4) as usize + 3], 0, "the Overworld's chunk (0, 0) isn't in it");
        // Centre follows the player there too, at an eighth.
        s.act(Action::Centre, Some(&h));
        s.screen(size, [0.0, 0.0], Some(&h), &measure);
        assert_eq!(s.map.centre, [20.0, 10.0]);
        // A waypoint made on it is the Nether's.
        s.create_at_pointer([200.0, 100.0], Some(&h), None);
        let Mode::Edit(e) = &s.mode else { panic!() };
        assert_eq!(e.dimension, NETHER);
        // The End, then back to the player's own: it follows them again.
        s.open_map(Some(&h), None);
        s.act(Action::CycleMapDimension, Some(&h));
        assert_eq!(s.shown_dimension(), END);
        assert_eq!(s.map_cut(), Cut::Surface);
        s.act(Action::CycleMapDimension, Some(&h));
        assert_eq!(s.shown_dimension(), OVERWORLD);
        assert!(s.map_shown.is_none() && s.map.follow);
        assert_eq!(s.map_cut(), s.cut);
        // Nothing was written into the Nether's folder by looking.
        assert!(!map_dir(&data, "world-x", NETHER).join("surface").exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_region_that_cant_be_written_is_kept_and_tried_again() {
        let base = std::env::temp_dir().join(format!("atlas-test-unwritable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // The map's folder is a file, so no region can be written under it.
        let dir = base.join("map");
        std::fs::write(&dir, b"not a folder").unwrap();
        let mut store = Store::default();
        store.open(Some(dir.clone()));
        store.put(0, 0, grass(70));
        assert!(!store.save(), "the failed write is reported");
        assert!(store.regions.values().all(|r| r.dirty), "and the region still counts as changed");
        // Filling the store past its limit forgets no unsaved region.
        for i in 1..MAX_REGIONS as i32 + 5 {
            store.put(i * REGION, 0, grass(70));
        }
        assert_eq!(store.regions_loaded(), MAX_REGIONS + 5);
        assert_eq!(store.column(3, 3), Some((LAND | 0x5D_A040, 70, 0, 0)));
        // Once the folder can be written, the next save writes everything.
        std::fs::remove_file(&dir).unwrap();
        assert!(store.save());
        assert!(store.regions.values().all(|r| !r.dirty));
        let mut again = Store::default();
        again.open(Some(dir.clone()));
        assert_eq!(again.column(3, 3), Some((LAND | 0x5D_A040, 70, 0, 0)));
        assert!(again.has((MAX_REGIONS as i32 + 4) * REGION, 0));
        // And with the folder writable, trimming forgets saved regions again.
        for i in 0..MAX_REGIONS as i32 + 5 {
            again.put(i * REGION, REGION, grass(71));
        }
        assert!(again.regions_loaded() <= MAX_REGIONS);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_minimap_grid_is_fixed_to_the_world_and_shifts_smoothly() {
        // North up, 1 block a texel: moving a tenth of a block shifts the
        // shown area a tenth of a texel and keeps the grid.
        let a = mini_grid(100.0, 50.0, 0.0, false, 1.0, MINI);
        let b = mini_grid(100.1, 50.0, 0.0, false, 1.0, MINI);
        assert_eq!(a.origin, b.origin);
        assert!((b.shift[0] - a.shift[0] - 0.1).abs() < 1e-4);
        // The player is in the middle of what shows.
        let mid = |g: &MiniGrid| {
            let i = (g.shift[0] + (MINI as f32 - 1.0) / 2.0).floor() as usize;
            let j = (g.shift[1] + (MINI as f32 - 1.0) / 2.0).floor() as usize;
            g.block(i, j)
        };
        assert_eq!(mid(&a), (100, 50));
        // Crossing into the next block moves the grid by one.
        let c = mini_grid(101.0, 50.0, 0.0, false, 1.0, MINI);
        assert_eq!(c.origin[0], a.origin[0] + 1);
        assert_eq!(mid(&c), (101, 50));
        // Turning: where the player looks is up.
        let (right, down) = mini_axes(true, 0.0);
        // Facing south (+z): up on the map is +z, so down is -z; right is west (-x).
        assert_eq!(down.map(|v| v.round()), [0.0, -1.0]);
        assert_eq!(right.map(|v| v.round()), [-1.0, 0.0]);
        let g = mini_grid(0.0, 0.0, 0.0, true, 1.0, MINI);
        let ahead = g.to_screen(0.0, 10.0, g.shown());
        assert!(ahead[1] < -9.0 && ahead[0].abs() < 1e-3);
    }

    #[test]
    fn the_minimap_image_shades_like_a_map_and_rounds() {
        let mut live = Store::default();
        live.put(0, 0, grass(64));
        let grid = mini_grid(8.0, 8.0, 0.0, false, 1.0, MINI);
        let mut img = Vec::new();
        let how = Shading::default();
        let mut pixel = |x, z| {
            let mut column = |x, z| live.column(x, z);
            shaded(&mut column, x, z, 1, how)
        };
        compose_minimap(&grid, true, 78, &mut pixel, &mut img);
        assert_eq!(img.len(), MINI * MINI * 4);
        // The corner is outside the circle; the middle is the grass.
        assert_eq!(&img[0..4], &[0, 0, 0, 0]);
        let mid = (64 * MINI + 64) * 4;
        assert_eq!(img[mid + 3], 255);
        assert_ne!(&img[mid..mid + 4], &UNKNOWN);
        // Far off nothing was seen.
        let far = mini_grid(5000.0, 5000.0, 0.0, false, 1.0, MINI);
        compose_minimap(&far, false, 78, &mut pixel, &mut img);
        assert_eq!(&img[mid..mid + 3], &UNKNOWN[..3]);
        assert_eq!(img[mid + 3], 198);
    }

    #[test]
    fn the_minimap_draws_its_image_dots_waypoints_compass_and_lines() {
        let mut s = State::default();
        s.settings = Some(Settings {
            lock_north: true,
            ..Settings::default()
        });
        s.waypoints.push(Waypoint {
            name: "far".into(),
            pos: [1000, 64, 0],
            dimension: OVERWORLD.into(),
            color: 0xFF0000,
            hidden: false,
            ..Waypoint::default()
        });
        let h = Here {
            pos: [0.5, 64.0, 0.5],
            dimension: OVERWORLD.into(),
            yaw: 0.0,
        };
        let info = MiniInfo {
            biome: Some("minecraft:dark_forest".into()),
            day_time: Some(6000),
            ..MiniInfo::default()
        };
        // Nothing before the image was uploaded.
        let gui = [480.0, 270.0];
        assert!(s.minimap_hud(gui, [0.0, 0.0], &h, &info, &[], &measure).is_empty());
        assert!(s.update_minimap(0.5, 0.5, 0.0));
        assert!(!s.update_minimap(0.6, 0.5, 0.0));
        s.mini_ready = true;
        let dots = [
            Dot { pos: [5.0, 64.0, 0.0], kind: DotKind::Mob, ..Dot::default() },
            Dot { pos: [900.0, 64.0, 0.0], kind: DotKind::Item, ..Dot::default() },
        ];
        let draws = s.minimap_hud(gui, [0.0, 0.0], &h, &info, &dots, &measure);
        let mut texts: Vec<&str> = draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text(t, ..) | Draw::Label(t, ..) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        // Each compass letter is drawn five times: four for its outline.
        assert_eq!(facing_text(0.0), "South");
        assert_eq!(facing_text(-90.0), "East");
        texts.dedup();
        assert_eq!(texts, ["F", "N", "E", "S", "W", "0, 64, 0", "Dark Forest", "Day 1, 12:00"]);
        assert!(draws.iter().any(|d| matches!(d, Draw::ModImage(..))));
        // The near mob's dot shows; the far item's doesn't.
        let yellow = draws.iter().filter(|d| matches!(d, Draw::Quad(_, c) if *c == DotKind::Mob.color())).count();
        let red = draws.iter().filter(|d| matches!(d, Draw::Quad(_, c) if *c == DotKind::Item.color())).count();
        assert_eq!((yellow, red), (1, 0));
        // The far waypoint's letter sits on the right edge.
        let at = draws
            .iter()
            .find_map(|d| match d {
                Draw::Label(t, at, ..) if t == "F" => Some(*at),
                _ => None,
            })
            .unwrap();
        let [bw, _] = s.minimap_box(gui, &h, &info, &measure);
        assert!(at[0] > bw / 2.0 + s.mini_side(gui) / 2.0 - 8.0, "{at:?}");
        // A share of the window's height, whatever its size.
        assert_eq!(s.mini_side(gui), 67.0);
        assert_eq!(s.mini_side([200.0, 120.0]), 30.0);
    }

    #[test]
    fn modes_choose_the_cut_and_cuts_keep_their_own_maps() {
        assert_eq!(cut_for(MapMode::Auto, 64, 70, false, false), Cut::Surface);
        assert_eq!(cut_for(MapMode::Auto, 64, 20, true, false), Cut::Below(31));
        assert_eq!(cut_for(MapMode::Auto, 64, 100, false, true), Cut::Below(111));
        assert_eq!(cut_for(MapMode::Cave, 64, -3, false, false), Cut::Below(-1));
        assert_eq!(cut_for(MapMode::Layer, 40, 100, false, false), Cut::Below(40));
        let mut maps = Maps::default();
        maps.store(Cut::Surface).put(0, 0, grass(64));
        assert!(maps.store(Cut::Surface).column(3, 3).is_some());
        assert!(maps.store(Cut::Below(31)).column(3, 3).is_none());
        // More cuts than are kept open: the surface stays.
        for y in 0..6 {
            maps.store(Cut::Below(y * 16 + 15));
        }
        assert!(maps.store(Cut::Surface).column(3, 3).is_some());
    }

    #[test]
    fn seeds_are_typed_per_world() {
        assert_eq!(parse_seed(" 42 "), Some(42));
        assert_eq!(parse_seed("-7"), Some(-7));
        // Java: "hello".hashCode() == 99162322
        assert_eq!(parse_seed("hello"), Some(99_162_322));
        assert_eq!(parse_seed(""), None);
        let dir = std::env::temp_dir().join(format!("atlas-seed-{}", std::process::id()));
        let mut s = State::default();
        assert_eq!(s.command("seed 5"), "Atlas: join a world first");
        s.load_world(Some("world-a".into()), dir.clone());
        assert!(s.command("seed 12345").starts_with("Atlas: seed 12345"));
        s.load_world(Some("world-b".into()), dir.clone());
        assert_eq!(s.seed, None);
        s.load_world(Some("world-a".into()), dir.clone());
        assert_eq!(s.seed, Some(12345));
        assert_eq!(s.command("slime on"), "Atlas: slime on");
        assert!(s.settings().slime && s.take_changed_settings().is_some());
        assert_eq!(s.command("seed off"), "Atlas: seed forgotten");
        assert_eq!(s.seed, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn icons_show_while_tab_is_held() {
        let mut s = State::default();
        let h = Here { pos: [0.5, 64.0, 0.5], dimension: OVERWORLD.into(), yaw: 0.0 };
        s.update_minimap(0.5, 0.5, 0.0);
        s.mini_ready = true;
        s.face_slot = Some(4);
        s.faces.insert("minecraft:zombie".into(), [0.0, 0.0, 8.0, 8.0]);
        let dots = [
            Dot { pos: [3.0, 64.0, 0.0], icon: Icon::Mob("minecraft:zombie".into()), ..Dot::default() },
            Dot { pos: [0.0, 64.0, 3.0], kind: DotKind::Item, icon: Icon::Item("minecraft:apple".into()), ..Dot::default() },
            Dot { pos: [3.0, 64.0, 3.0], kind: DotKind::Player, icon: Icon::Skin("skins/x".into()), ..Dot::default() },
        ];
        let info = MiniInfo::default();
        let gui = [480.0, 270.0];
        let count = |s: &State| {
            let d = s.minimap_hud(gui, [0.0, 0.0], &h, &info, &dots, &measure);
            let items = d.iter().filter(|x| matches!(x, Draw::Item(..))).count();
            let skins = d.iter().filter(|x| matches!(x, Draw::Skin(..))).count();
            let faces = d.iter().filter(|x| matches!(x, Draw::Image(4, ..))).count();
            (items, skins, faces)
        };
        assert_eq!(count(&s), (0, 0, 0));
        s.tab_held = true;
        assert_eq!(count(&s), (1, 1, 1));
    }

    #[test]
    fn overlays_mark_chunks_and_dark_blocks() {
        let mut s = State::default();
        s.maps.store(Cut::Surface).put(0, 0, grass(64));
        s.settings = Some(Settings { lock_north: true, ..Settings::default() });
        s.update_minimap(8.5, 8.5, 0.0);
        let plain = s.mini_image.clone();
        let mut st = s.settings();
        st.chunk_grid = 3;
        s.change_settings(st);
        assert!(s.update_minimap(8.5, 8.5, 0.0));
        assert_ne!(s.mini_image, plain, "the grid shows");
        st.chunk_grid = 0;
        st.light_overlay = true;
        s.change_settings(st);
        assert!(s.update_minimap_lit(8.5, 8.5, 0.0, &|_, _, _| 0));
        assert_ne!(s.mini_image, plain, "dark blocks show");
        s.change_settings(Settings { light_level: 0, ..st });
        assert!(s.update_minimap_lit(8.5, 8.5, 0.0, &|_, _, _| 0));
        assert_eq!(s.mini_image, plain);
    }

    #[test]
    fn block_map_settings_shade_the_map() {
        let (flat, strong) = (
            Shading { slopes: 0, terrain_depth: false, ..Shading::default() },
            Shading { slopes: 2, terrain_depth: false, ..Shading::default() },
        );
        let at = |how, n, nw| shade(LAND | 0x808080, 70, n, nw, 1, 0, how)[0];
        let flat_at = at(flat, Some(68), Some(68));
        assert_eq!(flat_at, 137, "the grade alone");
        let normal = Shading { slopes: 1, ..flat };
        assert!(at(normal, Some(69), Some(69)) > flat_at, "facing north: lit");
        assert!(at(strong, Some(69), Some(69)) > at(normal, Some(69), Some(69)));
        // Facing south is in shadow, facing east or west half so.
        let (south, east) = (at(strong, Some(71), Some(71)), at(strong, Some(70), Some(69)));
        assert!(south < east && east < flat_at);
        // Xaero's, measured (relative to flat ground): north neighbour,
        // north west neighbour, how bright.
        for (n, nw, xaero) in [
            (69, 69, 1.18),
            (71, 71, 0.578),
            (70, 71, 0.843),
            (70, 69, 0.843),
            (70, 73, 0.717),
            (70, 66, 0.684),
            (71, 75, 0.578),
            (69, 64, 0.738),
            (66, 66, 1.054),
            (38, 38, 1.0),
        ] {
            let got = slope_light(70, Some(n), Some(nw), 1);
            assert!((got - xaero).abs() <= 0.035, "{n} {nw}: {got} against {xaero}");
        }
        assert_eq!(slope_light(70, None, None, 1), 1.0);
        // A level of the world map further out: steps over its distance.
        assert_eq!(slope_light(70, Some(66), Some(66), 4), slope_light(70, Some(69), Some(69), 1));
        // Land is even from sea level up, darker below it with Terrain Depth.
        let depth = Shading { terrain_depth: true, ..flat };
        let high = |how, y| shade(LAND | 0x808080, y, None, None, 1, 0, how)[0];
        assert_eq!(high(depth, 63), high(depth, 200));
        assert!(high(depth, 60) < high(depth, 63));
        assert_eq!(high(flat, 60), high(flat, 63));
        // Water's depth was blended in when read: graded as land here.
        let water = |how| shade(WATER | 0x808080, 60, None, None, 1, 0, how)[0];
        assert_eq!(water(flat), flat_at);
        // Lit by the sky or block light, as Xaero's at midnight: 9 24ths
        // in the dark, a 24th more for each level of light.
        let night = Shading { sky: 0.0, ..flat };
        let lit = |l| shade(LAND | 0x808080, 70, None, None, 1, l, night)[0];
        assert_eq!(lit(0), (137.2f32 * 9.0 / 24.0).round() as u8);
        assert_eq!(lit(13), (137.2f32 * 22.0 / 24.0).round() as u8);
        assert_eq!(lit(15), flat_at);
        assert_eq!(shade(LAND | 0x808080, 70, None, None, 1, 0, Shading { lighting: false, ..night })[0], flat_at);
        assert_eq!(shade(LAND | 0x808080, 70, None, None, 1, 0, Shading { sky: 15.0, ..night })[0], flat_at);
        // The time of day reshades the surface, not cuts.
        let mut s = State::default();
        s.set_day_time(Some(18000));
        assert_eq!(s.shading(Cut::Surface).sky, 0.0);
        assert_eq!(s.shading(Cut::Below(31)).sky, 15.0);
        let key = TileKey { cut: Cut::Surface, level: 0, x: 0, z: 0 };
        let dark = s.tile_stamp(key);
        s.set_day_time(Some(6000));
        assert_ne!(s.tile_stamp(key), dark);
        s.set_day_time(None);
        assert_eq!(s.sky, 15.0, "no clock, no sky: day");
        // Auto waits for the timer; other modes don't.
        let mut s = State::default();
        s.want_cut(Cut::Below(31));
        assert_eq!(s.cut, Cut::Surface);
        s.change_settings(Settings { cave_timer: 0, ..s.settings() });
        s.want_cut(Cut::Below(31));
        assert_eq!(s.cut, Cut::Below(31));
    }

    #[test]
    fn dots_fade_with_height() {
        assert_eq!(height_fade(0.0, 8.0), 1.0);
        assert_eq!(height_fade(-8.0, 8.0), 1.0, "near heights don't fade");
        assert!(height_fade(24.0, 8.0) > 0.4 && height_fade(24.0, 8.0) < 0.6);
        assert_eq!(height_fade(45.0, 8.0), 0.0);
        assert_eq!(fade(0xFFFF_DD33, 1.0, 1.0), 0xFFFF_DD33);
        let far = fade(0xFFFF_DD33, 0.35, 0.0);
        assert_eq!(far >> 24, 77);
        assert!((far >> 16 & 255) < 100);
    }

    #[test]
    fn dots_show_on_the_level_the_map_shows() {
        let mut s = State::default();
        s.maps.store(Cut::Surface).put(0, 0, grass(64));
        let at = |y: f64| Dot { pos: [5.0, y, 5.0], kind: DotKind::Mob, ..Dot::default() };
        let kept = |s: &mut State, y: f64| !s.visible_dots(vec![at(y)], 65.0).is_empty();
        assert!(kept(&mut s, 65.0));
        assert!(!kept(&mut s, 20.0), "a cave mob under the grass");
        s.set_cut(Cut::Below(31));
        assert!(kept(&mut s, 20.0));
        assert!(!kept(&mut s, 65.0));
    }

    #[test]
    fn sets_deathpoints_and_temporary_waypoints() {
        let dir = std::env::temp_dir().join(format!("atlas-sets-{}", std::process::id()));
        let mut s = State::default();
        s.load_world(Some("w".into()), dir.clone());
        let h = here(OVERWORLD);
        s.open_create(&h, Back::List, None);
        s.act(Action::Save, None);
        assert_eq!(s.waypoints[0].set, DEFAULT_SET);
        // A new set is empty, and what is in another set doesn't show.
        s.list_filter = "Mine".into();
        s.act(Action::NewSet, None);
        assert_eq!(s.set, "Mine");
        let camera = Camera { pos: h.pos, yaw: 0.0, pitch: 0.0, fov: 70.0 };
        assert!(!s.shows(&s.waypoints[0].clone()));
        s.open_create(&h, Back::List, None);
        s.act(Action::Save, None);
        assert_eq!(s.sets(), [DEFAULT_SET, "Mine"]);
        let _ = camera;
        // Clearing the set takes two clicks and leaves the others.
        s.act(Action::ClearSet, None);
        assert_eq!(s.waypoints.len(), 2);
        s.act(Action::ClearSet, None);
        assert_eq!(s.waypoints.len(), 1);
        s.act(Action::CycleSet, None);
        assert_eq!(s.set, DEFAULT_SET);
        // A deathpoint comes once, and goes when reached.
        s.track_death(true, &h);
        s.track_death(true, &h);
        let deaths = |s: &State| s.waypoints.iter().filter(|w| w.kind == WpKind::Death).count();
        assert_eq!(deaths(&s), 1);
        let far = Here { pos: [500.0, 64.0, 500.0], ..h.clone() };
        s.track_death(false, &far);
        assert_eq!(deaths(&s), 1);
        s.track_death(false, &h);
        assert_eq!(deaths(&s), 0);
        // Temporary waypoints aren't kept; sets and deathpoints are.
        s.add_temporary(&h);
        s.track_death(true, &far);
        let back = decode(&encode(&s.waypoints));
        assert_eq!(back.len(), s.waypoints.len() - 1);
        assert!(back.iter().any(|w| w.kind == WpKind::Death));
        s.act(Action::Share(0), None);
        assert!(s.take_say().is_some_and(|l| l.contains("Death")));
        // Settings that act when something happens.
        s.can_teleport = true;
        s.waypoints[0].dimension = NETHER.into();
        s.change_settings(Settings { cross_dim_tp: false, ..s.settings() });
        s.act(Action::Teleport(0), Some(&h));
        assert!(s.teleport.is_none(), "not across dimensions");
        s.change_settings(Settings { cross_dim_tp: true, new_to_bottom: true, ..s.settings() });
        s.act(Action::Teleport(0), Some(&h));
        assert!(s.teleport.is_some());
        s.add_temporary(&h);
        assert_eq!(s.waypoints.last().unwrap().kind, WpKind::Temporary);
        s.change_settings(Settings { deathpoints: false, ..s.settings() });
        let before = s.waypoints.len();
        s.track_death(false, &far);
        s.track_death(true, &far);
        assert_eq!(s.waypoints.len(), before, "no deathpoint when off");
        s.track_death(false, &far);
        s.change_settings(Settings { deathpoints: true, keep_deathpoints: false, ..s.settings() });
        s.track_death(true, &far);
        s.track_death(false, &h);
        s.track_death(true, &h);
        let deaths = s.waypoints.iter().filter(|w| w.kind == WpKind::Death).count();
        assert_eq!(deaths, 1, "old ones go");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slime_chunks_follow_the_seed() {
        // About one chunk in ten.
        let n = (0..100).flat_map(|x| (0..100).map(move |z| (x, z))).filter(|&(x, z)| is_slime_chunk(12345, x, z)).count();
        assert!((800..1200).contains(&n), "{n}");
        assert_ne!(
            (0..64).map(|x| is_slime_chunk(1, x, 0)).collect::<Vec<_>>(),
            (0..64).map(|x| is_slime_chunk(2, x, 0)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn settings_round_trip_and_commands_set_them() {
        let s = Settings::read(|_| None);
        assert_eq!(s, Settings::default());
        let mut s = s;
        assert_eq!(s.command("size 30"), ("Atlas: size 30".to_string(), true));
        assert!(!s.command("size 120").1);
        assert!(s.command("mode layer -20").1);
        assert!(!s.command("mode layer high").1);
        assert!(s.command("zoom 2").1);
        assert!(s.command("shape round").1);
        assert!(s.command("minimap off").1);
        assert_eq!(s.mode, MapMode::Layer);
        assert_eq!(s.layer_y, -20);
        let text = s.to_text();
        let back = Settings::read(|k: &str| (k == SETTINGS_FILE).then(|| text.clone()));
        assert_eq!(back, s);
        assert!(!back.minimap && back.round && back.size == 30);
        let i = Opt::index("zoom").unwrap();
        assert_eq!(OPTS[i].text(back.raw(i)), "Blocks per Pixel: 2");
        let mut back = back;
        assert_eq!(back.command("zoom").0, "Atlas: zoom is 2");
        // Options > Mods: a title that does nothing, then the main switches.
        let mut s = Settings::default();
        s.click(0);
        assert_eq!(s, Settings::default());
        s.click(1);
        assert!(!s.minimap);
    }

    #[test]
    fn the_enlarged_minimap_and_zoom_keys_work() {
        let mut s = State::default();
        let gui = [480.0, 270.0];
        let small = s.mini_side(gui);
        s.enlarge_key(true);
        assert!(s.mini_side(gui) > small && !s.turning());
        s.enlarge_key(false);
        assert_eq!(s.mini_side(gui), small);
        assert!(s.turning());
        // Toggled, it stays on after the key comes up.
        s.change_settings(Settings { toggle_enlarged: true, ..s.settings() });
        s.enlarge_key(true);
        s.enlarge_key(false);
        assert!(s.enlarged);
        s.enlarge_key(true);
        assert!(!s.enlarged);
        s.change_settings(Settings { toggle_enlarged: false, ..s.settings() });
        s.zoom_key(true);
        assert_eq!(s.settings().zoom, 0.5);
        s.zoom_key(false);
        s.zoom_key(false);
        assert_eq!(s.settings().zoom, 2.0);
        s.set_cut(Cut::Below(31));
        assert_eq!(s.mini_zoom(), 1.0, "closer in caves");
        assert_eq!(sky_light(6000), 15.0);
        assert_eq!(sky_light(1000), 15.0, "after sunrise");
        assert_eq!(sky_light(18000), 0.0);
        // Measured from Xaero's at 12500: 5.1.
        assert!((sky_light(12500) - 5.5).abs() <= 0.5);
        assert!(sky_light(12000) > sky_light(12500) && sky_light(12500) > sky_light(13000));
    }

    #[test]
    fn every_setting_reads_back_what_it_stores() {
        for (i, o) in OPTS.iter().enumerate() {
            let v = Settings::default().raw(i);
            assert_eq!(o.parse(&o.stored(v)), Ok(v), "{}", o.key);
            assert_ne!(o.next(v), v, "{}", o.key);
        }
    }

    /// Everything the logic draws and decides, for settings `st`.
    fn everything(st: Settings) -> String {
        let h = Here { pos: [8.5, 100.0, 8.5], dimension: OVERWORLD.into(), yaw: 30.0 };
        let info = MiniInfo {
            biome: Some("minecraft:plains".into()),
            day_time: Some(18000),
            light: Some((3, 9)),
            weather: Some((0.0, 0.0)),
        };
        let dots = [
            Dot { pos: [12.0, 100.0, 8.0], kind: DotKind::Mob, icon: Icon::Mob("minecraft:zombie".into()), name: Some("Zombie".into()) },
            Dot { pos: [4.0, 126.0, 8.0], kind: DotKind::Player, icon: Icon::Skin("s".into()), name: Some("P".into()) },
            Dot { pos: [8.0, 236.0, 12.0], kind: DotKind::Item, icon: Icon::Item("minecraft:apple".into()), name: Some("Apple".into()) },
            Dot { pos: [8.0, 102.0, 4.0], kind: DotKind::Other, icon: Icon::None, name: Some("Boat".into()) },
        ];
        let camera = Camera { pos: [8.5, 101.6, 8.5], yaw: 0.0, pitch: 5.0, fov: 70.0 };
        let gui = [480.0, 270.0];
        let mut out = String::new();
        for (enlarged, cut, tab, picked) in [
            (false, Cut::Surface, false, None),
            (true, Cut::Surface, true, Some(0)),
            (false, Cut::Below(31), true, Some(0)),
        ] {
            let mut s = State::default();
            s.settings = Some(st);
            s.seed = Some(12345);
            // Night, for Lighting.
            s.set_day_time(Some(18000));
            s.enlarged = enlarged;
            s.tab_held = tab;
            s.set_cut(cut);
            s.can_teleport = true;
            s.selected = picked;
            s.faces.insert("minecraft:zombie".into(), [0.0, 0.0, 8.0, 8.0]);
            s.face_slot = Some(1);
            let mut t = grass(100);
            t.height[5] = 106;
            // Under sea level, for Terrain Depth.
            t.height[40] = 50;
            t.color[9] = WATER | 0x3F_76E4;
            t.depth[9] = 9;
            s.maps.store(cut).put(0, 0, t);
            // A portal under the player, and the server's view distance.
            s.put_portals((0, 0), NETHER_PORTAL);
            s.view_distance = Some(1);
            s.put_new_chunk((0, 0), true);
            s.put_new_chunk((-1, 0), false);
            for (i, set) in ["a", "b"].into_iter().enumerate() {
                s.waypoints.push(Waypoint {
                    name: "Wp".into(),
                    // 120 blocks off and 11 degrees to the side.
                    pos: [32 + i as i32, 102, 128 + i as i32 * 900],
                    set: if i == 0 { DEFAULT_SET.into() } else { set.into() },
                    ..Waypoint::default()
                });
            }
            s.update_minimap_lit(8.5, 8.5, 30.0, &|_, _, _| 4);
            s.mini_ready = true;
            let image: u64 = s.mini_image.iter().fold(7, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(*b)));
            let shown = s.visible_dots(dots.to_vec(), 100.0);
            let hud = s.minimap_hud(gui, [0.0, 0.0], &h, &info, &shown, &measure);
            let labels = s.labels(camera, h.pos, OVERWORLD, gui, &measure);
            s.open_list(None);
            let list = s.screen(gui, [0.0, 0.0], Some(&h), &measure).0;
            s.open_map(Some(&h), None);
            let map = s.screen(gui, [0.0, 0.0], Some(&h), &measure).0;
            let tile = s.compose_tile(TileKey { cut, level: 0, x: 0, z: 0 });
            let tile: u64 = tile.iter().fold(7, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(*b)));
            out.push_str(&format!("{image} {tile} {hud:?} {labels:?} {list:?} {map:?} {} {}", s.mini_n, s.mini_reach()));
        }
        out
    }

    #[test]
    fn every_setting_does_something() {
        // Those the game side reads: it must read them.
        let bridge = std::fs::read_to_string("mixins/atlas/bridge.rs").unwrap();
        const GAME_SIDE: [(&str, &str); 14] = [
            ("transparency", "settings.transparency"),
            ("minimap", "settings.minimap"),
            ("corner", "settings.corner"),
            ("hide_under_screen", "settings.hide_under_screen"),
            ("hide_under_f3", "settings.hide_under_f3"),
            ("center_enlarged", "settings.center_enlarged"),
            ("color_mode", "settings.color_mode"),
            ("auto_cave", "settings.auto_cave"),
            ("cave_nether", "settings.cave_nether"),
            ("cave_depth", "settings.cave_depth"),
            ("flowers", "settings.flowers"),
            ("plants", "settings.plants"),
            ("leaves", "settings.leaves"),
            ("stained_glass", "settings.stained_glass"),
        ];
        let more = ["hide_invisible", "entity_limit"];
        let mut dead = Vec::new();
        for (key, use_of) in GAME_SIDE {
            assert!(Opt::index(key).is_some(), "{key}");
            if !bridge.contains(use_of) {
                dead.push(key);
            }
        }
        for key in more {
            if !bridge.contains(&format!("settings.{key}")) {
                dead.push(key);
            }
        }
        // Behaviour over time, covered by their own tests.
        let timed = [
            "toggle_enlarged",
            "cave_timer",
            "deathpoints",
            "keep_deathpoints",
            "delete_reached",
            "new_to_bottom",
            "cross_dim_tp",
            // Only in the Nether: its own test.
            "ow_in_nether",
        ];
        // All the rest must change what is drawn.
        let base = Settings {
            chunk_grid: 1,
            light_overlay: true,
            light_level: 8,
            slime: true,
            names: 2,
            entity_height: true,
            facing: true,
            dimension_line: true,
            chunk_line: true,
            light_line: true,
            weather_line: true,
            mode: MapMode::Layer,
            ..Settings::default()
        };
        for (i, o) in OPTS.iter().enumerate() {
            // Settings about the distance need it shown, or looked for.
            let base = match o.key {
                "distance_scale" | "name_above" => Settings { distance_mode: 2, ..base },
                "angle_v" => Settings { angle_h: 180, ..base },
                _ => base,
            };
            let plain = everything(base);
            let skip = GAME_SIDE.iter().any(|g| g.0 == o.key) || more.contains(&o.key) || timed.contains(&o.key);
            if skip {
                continue;
            }
            let mut tries = vec![o.next(base.raw(i))];
            if let Kind::Slider { min, max, .. } = o.kind {
                tries.extend([min, max, (min + max) / 2, min + 1]);
            }
            if let Kind::Choice(names) = o.kind {
                tries.extend(0..names.len() as i32);
            }
            let works = tries.into_iter().filter(|v| *v != base.raw(i)).any(|v| {
                let mut st = base;
                st.put(i, v);
                everything(st) != plain
            });
            if !works {
                dead.push(o.key);
            }
        }
        assert!(dead.is_empty(), "settings that do nothing: {dead:?}");
    }

    #[test]
    fn the_settings_screen_pages_and_searches() {
        let mut s = State::default();
        s.open_settings(Page::Main);
        let size = [427.0, 240.0];
        let find = |w: &[Widget], a: &Action| w.iter().find(|x| x.action == *a).map(|x| x.rect);
        let (_, w) = s.screen(size, [0.0, 0.0], None, &measure);
        let view = find(&w, &Action::GoPage(Page::View)).unwrap();
        s.press(0, [view[0] + 1.0, view[1] + 1.0], &w, None);
        assert!(matches!(&s.mode, Mode::Settings(v) if v.page == Page::View));
        // A slider follows the pointer while dragged.
        let size_opt = Opt::index("size").unwrap();
        let (_, w) = s.screen(size, [0.0, 0.0], None, &measure);
        let slider = w.iter().find(|x| matches!(x.action, Action::Slide(i, _) if i == size_opt)).unwrap().rect;
        s.press(0, [slider[0] + slider[2] - 4.0, slider[1] + 5.0], &w, None);
        assert_eq!(s.settings().size, 60);
        assert!(s.drag(0, [-(slider[2] - 8.0), 0.0]));
        assert_eq!(s.settings().size, 10);
        s.release(0);
        assert!(s.take_changed_settings().is_some());
        // Search goes across the pages; Escape clears it, then goes back.
        let (_, w) = s.screen(size, [0.0, 0.0], None, &measure);
        let search = find(&w, &Action::FocusSearch).unwrap();
        s.press(0, [search[0] + 1.0, search[1] + 1.0], &w, None);
        for c in "slime".chars() {
            s.char_typed(c);
        }
        let (_, w) = s.screen(size, [0.0, 0.0], None, &measure);
        assert!(find(&w, &Action::Opt(Opt::index("slime").unwrap())).is_some());
        s.key("key.keyboard.escape", false, None, KEYS);
        s.key("key.keyboard.escape", false, None, KEYS);
        assert!(matches!(&s.mode, Mode::Settings(v) if v.page == Page::Main));
        s.key(DEFAULT_SETTINGS, false, None, KEYS);
        assert!(s.take_close());
    }

    #[test]
    fn the_world_map_pans_zooms_and_makes_waypoints_where_you_click() {
        let mut s = State::default();
        let h = here(OVERWORLD);
        s.open_map(Some(&h), Some('m'));
        assert_eq!(s.map.centre, [10.7, -3.2]);
        let size = [400.0, 200.0];
        s.screen_size = size;
        // Zooming keeps the block under the pointer.
        let at = [300.0, 50.0];
        let before = s.map.world(at, size);
        s.scroll_by(3.0, at);
        let after = s.map.world(at, size);
        assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
        assert!(s.map.bpp < 1.0);
        // Dragging moves it and stops following.
        s.press(0, [200.0, 100.0], &[], Some(&h));
        assert!(s.drag(0, [10.0, 0.0]));
        assert!(!s.map.follow);
        s.release(0);
        assert!(!s.drag(0, [10.0, 0.0]));
        // A right click opens the form there, and Save goes back to the map.
        s.map = MapView {
            centre: [0.0, 0.0],
            bpp: 1.0,
            follow: false,
            ..MapView::default()
        };
        // B over the map opens the form there, as a right click does.
        s.screen(size, [210.0, 80.0], Some(&h), &measure);
        s.key(DEFAULT_CREATE, false, Some(&h), KEYS);
        let Mode::Edit(e) = &s.mode else { panic!() };
        assert_eq!(e.coords, ["10".to_string(), "64".into(), "-20".into()]);
        s.act(Action::Cancel, None);
        s.press(1, [210.0, 80.0], &[], Some(&h));
        let Mode::Edit(e) = &s.mode else { panic!() };
        assert_eq!(e.coords, ["10".to_string(), "64".into(), "-20".into()]);
        assert_eq!(e.back, Back::Map);
        s.act(Action::Save, None);
        assert_eq!(s.mode, Mode::Map);
        assert_eq!(s.waypoints[0].pos, [10, 64, -20]);
        // Its marker is drawn on the map and clicking it edits it.
        let (_, widgets) = s.screen(size, [210.0, 80.0], Some(&h), &measure);
        assert_eq!(hit(&widgets, [210.5, 80.5]), Some(Action::MapWaypoint(0)));
        s.press(0, [210.5, 80.5], &widgets, Some(&h));
        assert!(matches!(&s.mode, Mode::Edit(e) if e.index == Some(0) && e.back == Back::Map));
        // Esc in the form goes back to the map; on the map it closes.
        s.key("key.keyboard.escape", false, None, KEYS);
        assert_eq!(s.mode, Mode::Map);
        s.key("key.keyboard.escape", false, None, KEYS);
        assert!(s.take_close());
    }

    #[test]
    fn world_map_tiles_cover_the_screen_at_a_pixel_or_more() {
        assert_eq!(map_level(0.25), 0);
        assert_eq!(map_level(1.0), 0);
        assert_eq!(map_level(1.5), 1);
        assert_eq!(map_level(8.0), 3);
        let view = MapView {
            centre: [0.0, 0.0],
            bpp: 2.0,
            ..MapView::default()
        };
        let tiles = view.tiles([400.0, 200.0], Cut::Surface);
        // 800 x 400 blocks at 256 blocks a tile: x -400..400, z -200..200.
        assert_eq!(tiles.len(), 4 * 2);
        let (key, dst) = tiles[0];
        assert_eq!((key.level, key.x, key.z), (1, -2, -1));
        assert_eq!(dst[2], 128.0);
        // An image shows what the store saw, transparent elsewhere.
        let mut store = Store::default();
        store.put(0, 0, grass(64));
        let mut img = Vec::new();
        compose_map_tile(&mut store, TileKey { cut: Cut::Surface, level: 0, x: 0, z: 0 }, Shading::default(), &mut img);
        assert_eq!(img[3], 255);
        let o = (20 * 128 + 20) * 4;
        assert_eq!(img[o + 3], 0);
    }

    #[test]
    fn map_slot_ids_are_negative_and_stable() {
        let mut images = MapImages::default();
        let a = images.id(TileKey { cut: Cut::Surface, level: 0, x: 0, z: 0 });
        let b = images.id(TileKey { cut: Cut::Surface, level: 0, x: 1, z: 0 });
        assert!(a < 0 && b < 0 && a != b);
        assert_eq!(images.id(TileKey { cut: Cut::Surface, level: 0, x: 0, z: 0 }), a);
        assert!(a > FACES_SLOT_KEY && b > FACES_SLOT_KEY);
    }
}
        #[no_mangle]
pub extern "C" fn horizon_mod_init() {
    // Punto de entrada para inicializar el minimapa Atlas en Horizon
    
    
}
