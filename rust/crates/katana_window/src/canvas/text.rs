//! Text on a canvas: a font resolved to a face, a line shaped into glyphs, lines laid out in a
//! rectangle as QPainter::drawText lays them out, the metrics the painters measure with
//! (QFontMetricsF), and a glyph's outline. One shaping serves both backends, so a label measured
//! by the metrics is drawn where it was measured on a raster and in a PDF alike.
//!
//! HOW A FAMILY RESOLVES (decided here, the plan's section 4.10). The families a Font asks for
//! are tried in order against the machine's fonts (fontdb: fontconfig's folders on Linux, the
//! font folder on Windows), matched as Qt matches a family name - without regard to case; the
//! first the machine has is used, at the weight and slant asked for. When none is there, a font
//! that wants fixed pitch (QFont::TypeWriter, the command line's) takes the machine's generic
//! monospace face, else the first fixed-pitch face it has; and anything still unresolved - a font
//! that names no family, which is the UI's own face, included - takes the UI's sans, the face
//! egui draws the window's text in (Ubuntu-Light, embedded in the program, so it is there on every
//! machine). A glyph the chosen face lacks (a CJK name in a Latin face) is found in another face
//! by cosmic-text's fallback, as Qt falls back per character.
//!
//! REJECTED: cosmic-text's own family matching for the first step. It matches names exactly and,
//! for a family the machine lacks, falls through to its own list (Noto, Open Sans, DejaVu), so the
//! fallback would differ from machine to machine with no UI face at its end.
//!
//! The glyph outlines are unhinted: a drawing's text is scaled and turned with the view, where
//! hinting for one pixel size would distort it, and the PDF's embedded fonts are unhinted too.
//!
//! The engine (the font database, the shaper's caches) is one per thread, made on first use: the
//! window paints on one thread, a test binary's cases run on many, and a plot job paints on its
//! own, none of them waiting for another's lock. It takes up to a second to scan the machine's
//! fonts the first time on a thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use cosmic_text::fontdb;
use cosmic_text::{
    Attrs, Buffer, CacheKey, CacheKeyFlags, Command, Family, FontSystem, Metrics, Shaping, Style,
    SwashCache, Weight, Wrap,
};

use super::painter::{
    Elide, FillRule, Font, FontMetrics, FontSize, FontWeight, HAlign, Path, PointF, RectF,
    TextFlags, VAlign,
};

/// A screen's and a QImage's logical resolution, 96 dots an inch: what a font's points are turned
/// into pixels at (QImage's default dotsPerMeter, 3780, is 96.01; Qt rounds it to 96).
pub const SCREEN_DPI: f64 = 96.0;

/// The ellipsis an elided text ends in (QFontMetrics::elidedText's U+2026).
const ELLIPSIS: &str = "\u{2026}";

/// The name egui gives its embedded sans in FontDefinitions::default(): the UI's own face.
const UI_SANS: &str = "Ubuntu-Light";

/// A font's size in pixels of a device at `dpi` dots an inch: points at 72 an inch (QFont's
/// pointSizeF on a paint device of that resolution), or pixels as given.
pub fn font_pixels(font: &Font, dpi: f64) -> f64 {
    match font.size {
        FontSize::Points(points) => points * dpi / 72.0,
        FontSize::Pixels(pixels) => f64::from(pixels),
    }
}

/// The family `font` resolves to on this machine (see the module comment), or None when not even
/// the UI's sans could be loaded.
pub fn resolved_family(font: &Font) -> Option<String> {
    with_engine(|engine| engine.resolve(font).map(|resolved| resolved.family))
}

/// The outlines of `text` in `font` on a device at `dpi`, its baseline starting at `origin`
/// (QPainterPath::addText), filled by the non-zero rule as glyph outlines are.
pub fn text_path(font: &Font, dpi: f64, origin: PointF, text: &str) -> Path {
    let mut path = Path::new();
    path.fill_rule = FillRule::Winding;
    with_engine(|engine| {
        if let Some(shaped) = engine.shape(font, dpi, &one_line(text)) {
            engine.outline(&shaped, origin, &mut path);
        }
    });
    path
}

/// The text of one line: QPainter::drawText(point, text) does not break lines, so a line break
/// is a space.
pub(crate) fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

// ---- the engine ----------------------------------------------------------------------------------

/// A font resolved to a family the machine has, with the weight and slant to ask of it, and the
/// face that family resolves to (whose metrics are the line's).
#[derive(Clone, Debug, PartialEq)]
struct Resolved {
    family: String,
    weight: Weight,
    style: Style,
    face: fontdb::ID,
}

/// One glyph of a shaped line.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacedGlyph {
    pub(crate) font: fontdb::ID,
    pub(crate) weight: Weight,
    /// The size the glyph is drawn at, in pixels of the device.
    pub(crate) size: f32,
    pub(crate) id: u16,
    pub(crate) flags: CacheKeyFlags,
    /// Where the pen is when the glyph is drawn, from the line's start.
    pub(crate) pen: f64,
    /// The glyph's own offsets from the pen: across, and up from the baseline.
    pub(crate) x_offset: f64,
    pub(crate) rise: f64,
    /// How far the pen moves after it, letter spacing included.
    pub(crate) advance: f64,
    /// The bytes of the line's text the glyph draws.
    pub(crate) range: Range<usize>,
}

/// A line shaped in one font.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shaped {
    /// The text as drawn: capitalised when the font says so.
    pub(crate) text: String,
    pub(crate) glyphs: Vec<PlacedGlyph>,
    pub(crate) width: f64,
    pub(crate) ascent: f64,
    pub(crate) descent: f64,
    pub(crate) leading: f64,
}

struct Engine {
    system: FontSystem,
    swash: SwashCache,
    /// The UI's sans: its family name and weight, as loaded from egui's embedded font.
    ui_sans: Option<(String, Weight)>,
    /// A family asked for, lower case, to the name the machine has it under.
    families: HashMap<String, Option<String>>,
    /// The generic fixed-pitch family, once looked for.
    monospace: Option<Option<String>>,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

fn with_engine<R>(work: impl FnOnce(&mut Engine) -> R) -> R {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let engine = slot.get_or_insert_with(Engine::new);
        work(engine)
    })
}

fn weight_of(weight: FontWeight) -> Weight {
    match weight {
        FontWeight::Normal => Weight::NORMAL,
        FontWeight::DemiBold => Weight::SEMIBOLD,
        FontWeight::Bold => Weight::BOLD,
    }
}

impl Engine {
    fn new() -> Self {
        let mut system = FontSystem::new();
        let ui_sans = egui::FontDefinitions::default()
            .font_data
            .get(UI_SANS)
            .and_then(|data| {
                let bytes: Arc<dyn AsRef<[u8]> + Sync + Send> = Arc::new(data.font.to_vec());
                let ids = system
                    .db_mut()
                    .load_font_source(fontdb::Source::Binary(bytes));
                let id = ids.first().copied()?;
                let face = system.db().face(id)?;
                let family = face.families.first()?.0.clone();
                Some((family, face.weight))
            });
        Self {
            system,
            swash: SwashCache::new(),
            ui_sans,
            families: HashMap::new(),
            monospace: None,
        }
    }

    /// The name the machine has `asked` under, matched without regard to case as Qt matches a
    /// family; None when it has no such family.
    fn family_named(&mut self, asked: &str) -> Option<String> {
        let key = asked.to_lowercase();
        if let Some(found) = self.families.get(&key) {
            return found.clone();
        }
        let found = self.system.db().faces().find_map(|face| {
            face.families
                .iter()
                .find(|(name, _)| name.to_lowercase() == key)
                .map(|(name, _)| name.clone())
        });
        self.families.insert(key, found.clone());
        found
    }

    /// The machine's fixed-pitch family: its generic monospace, else the first fixed-pitch face's
    /// family by name (so the choice does not depend on the order the folders were read in).
    fn monospace_family(&mut self) -> Option<String> {
        if let Some(found) = &self.monospace {
            return found.clone();
        }
        let db = self.system.db();
        let generic = db
            .query(&fontdb::Query {
                families: &[Family::Monospace],
                ..fontdb::Query::default()
            })
            .and_then(|id| db.face(id))
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()));
        let found = generic.or_else(|| {
            db.faces()
                .filter(|face| face.monospaced)
                .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
                .min()
        });
        self.monospace = Some(found.clone());
        found
    }

    fn resolve(&mut self, font: &Font) -> Option<Resolved> {
        let asked = weight_of(font.weight);
        let style = if font.italic {
            Style::Italic
        } else {
            Style::Normal
        };
        let mut chosen = None;
        for family in &font.families {
            if let Some(found) = self.family_named(family) {
                chosen = Some((found, asked));
                break;
            }
        }
        if chosen.is_none() && font.fixed_pitch {
            chosen = self.monospace_family().map(|found| (found, asked));
        }
        if chosen.is_none() {
            // The UI's sans at its own weight, as egui draws the window's text, unless more is
            // asked for (a bold title takes the bold face where the machine has one).
            chosen = self.ui_sans.clone().map(|(family, own)| {
                let weight = if font.weight == FontWeight::Normal {
                    own
                } else {
                    asked
                };
                (family, weight)
            });
        }
        let (family, weight) = chosen?;
        let face = self.system.db().query(&fontdb::Query {
            families: &[Family::Name(&family)],
            weight,
            stretch: fontdb::Stretch::Normal,
            style,
        })?;
        Some(Resolved {
            family,
            weight,
            style,
            face,
        })
    }

    /// The ascent, descent and leading of `resolved`'s face at `size` pixels (QFontMetricsF's,
    /// from the face's own tables); descent positive, below the baseline.
    fn line_metrics(&mut self, resolved: &Resolved, size: f64) -> (f64, f64, f64) {
        let Some(face) = self.system.get_font(resolved.face, resolved.weight) else {
            return (0.0, 0.0, 0.0);
        };
        let metrics = face.metrics();
        if metrics.units_per_em == 0 {
            return (0.0, 0.0, 0.0);
        }
        let scale = size / f64::from(metrics.units_per_em);
        (
            f64::from(metrics.ascent) * scale,
            f64::from(metrics.descent).abs() * scale,
            f64::from(metrics.leading).max(0.0) * scale,
        )
    }

    /// `text` (one line) shaped in `font` on a device at `dpi`; None for a font of no size or one
    /// that resolves to nothing.
    fn shape(&mut self, font: &Font, dpi: f64, text: &str) -> Option<Shaped> {
        let pixels = font_pixels(font, dpi);
        if !pixels.is_finite() || pixels <= 0.0 {
            return None;
        }
        let resolved = self.resolve(font)?;
        let (ascent, descent, leading) = self.line_metrics(&resolved, pixels);
        let shown = if font.all_uppercase {
            text.to_uppercase()
        } else {
            String::from(text)
        };
        let size = pixels as f32;
        let mut buffer = Buffer::new_empty(Metrics::new(size, size));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        let attrs = Attrs::new()
            .family(Family::Name(&resolved.family))
            .weight(resolved.weight)
            .style(resolved.style)
            .cache_key_flags(CacheKeyFlags::DISABLE_HINTING);
        buffer.set_text(&shown, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.system, false);

        // QFont::PercentageSpacing: each glyph's advance taken at `percent` of itself.
        let stretch = font.letter_spacing_percent / 100.0;
        let mut glyphs = Vec::new();
        let mut shift = 0.0;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let own = f64::from(glyph.w);
                let em = f64::from(glyph.font_size);
                glyphs.push(PlacedGlyph {
                    font: glyph.font_id,
                    weight: glyph.font_weight,
                    size: glyph.font_size,
                    id: glyph.glyph_id,
                    flags: glyph.cache_key_flags,
                    pen: f64::from(glyph.x) + shift,
                    x_offset: em * f64::from(glyph.x_offset),
                    rise: em * f64::from(glyph.y_offset) - f64::from(glyph.y),
                    advance: own * stretch,
                    range: glyph.start..glyph.end,
                });
                shift += own * (stretch - 1.0);
            }
        }
        let width = glyphs
            .iter()
            .map(|glyph| glyph.pen + glyph.advance)
            .fold(0.0, f64::max);
        Some(Shaped {
            text: shown,
            glyphs,
            width,
            ascent,
            descent,
            leading,
        })
    }

    /// The outlines of `shaped`'s glyphs, its baseline starting at `origin`, added to `path`.
    fn outline(&mut self, shaped: &Shaped, origin: PointF, path: &mut Path) {
        for glyph in &shaped.glyphs {
            let (key, _, _) = CacheKey::new(
                glyph.font,
                glyph.id,
                glyph.size,
                (0.0, 0.0),
                glyph.weight,
                glyph.flags,
            );
            let Some(commands) = self.swash.get_outline_commands(&mut self.system, key) else {
                continue;
            };
            let x0 = origin.x + glyph.pen + glyph.x_offset;
            let y0 = origin.y - glyph.rise;
            // A font's outline runs y up; the canvas's y runs down.
            let at = |x: f32, y: f32| PointF::new(x0 + f64::from(x), y0 - f64::from(y));
            for command in commands {
                match command {
                    Command::MoveTo(p) => path.move_to(at(p.x, p.y)),
                    Command::LineTo(p) => path.line_to(at(p.x, p.y)),
                    Command::CurveTo(c1, c2, end) => {
                        path.cubic_to(at(c1.x, c1.y), at(c2.x, c2.y), at(end.x, end.y))
                    }
                    Command::QuadTo(control, end) => {
                        path.quad_to(at(control.x, control.y), at(end.x, end.y))
                    }
                    Command::Close => path.close_subpath(),
                }
            }
        }
    }

    /// The bytes of the font file a glyph's face is in, and the face's index in it: what the PDF
    /// embeds.
    fn face_data(&self, face: fontdb::ID) -> Option<(Vec<u8>, u32)> {
        self.system
            .db()
            .with_face_data(face, |data, index| (data.to_vec(), index))
    }
}

/// `text` shaped in `font` on a device at `dpi` (crate-private: the backends draw it).
pub(crate) fn shape_line(font: &Font, dpi: f64, text: &str) -> Option<Shaped> {
    with_engine(|engine| engine.shape(font, dpi, text))
}

/// The outlines of an already shaped line, its baseline starting at `origin`.
pub(crate) fn outline_of(shaped: &Shaped, origin: PointF) -> Path {
    let mut path = Path::new();
    path.fill_rule = FillRule::Winding;
    with_engine(|engine| engine.outline(shaped, origin, &mut path));
    path
}

/// The font file of a face this thread's engine resolved, and the face's index in it.
pub(crate) fn face_data(face: fontdb::ID) -> Option<(Vec<u8>, u32)> {
    with_engine(|engine| engine.face_data(face))
}

// ---- lines in a rectangle ---------------------------------------------------------------------

/// Text laid out in a rectangle: each line's baseline origin and glyphs, and the rectangle the
/// whole takes (drawText's boundingRect).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LaidOut {
    pub(crate) lines: Vec<(PointF, Shaped)>,
    pub(crate) bounds: RectF,
}

/// Words of `paragraph` set in lines no wider than `width` where a break between words allows,
/// a word wider than the line on a line of its own (Qt::TextWordWrap breaks only between words).
fn wrapped(font: &Font, dpi: f64, paragraph: &str, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in paragraph.split(' ') {
        let candidate = if line.is_empty() {
            String::from(word)
        } else {
            format!("{line} {word}")
        };
        let fits = shape_line(font, dpi, &candidate).is_none_or(|shaped| shaped.width <= width);
        if !fits && !line.is_empty() {
            lines.push(std::mem::replace(&mut line, String::from(word)));
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
}

/// `text` laid out in `rect` as QPainter::drawText(rect, flags, text) lays it out (Qt's
/// qt_format_text): its paragraphs one under another - a line break is a space under
/// Qt::TextSingleLine - each broken at words to the rectangle's width under Qt::TextWordWrap;
/// lines a font height apart with the font's leading between them; each line placed across the
/// rectangle and the block of lines down it as the flags align them. None when the font
/// resolves to nothing.
pub(crate) fn lay_out(
    font: &Font,
    dpi: f64,
    rect: RectF,
    flags: TextFlags,
    text: &str,
) -> Option<LaidOut> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let paragraphs: Vec<String> = if flags.single_line {
        vec![text.replace('\n', " ")]
    } else {
        text.split('\n').map(String::from).collect()
    };
    let mut shaped_lines = Vec::new();
    for paragraph in &paragraphs {
        let lines = if flags.word_wrap {
            wrapped(font, dpi, paragraph, rect.width)
        } else {
            vec![paragraph.clone()]
        };
        for line in lines {
            shaped_lines.push(shape_line(font, dpi, &line)?);
        }
    }
    let first = shaped_lines.first()?;
    let (ascent, descent, leading) = (first.ascent, first.descent, first.leading);
    let line_height = ascent + descent;
    let count = shaped_lines.len() as f64;
    let height = count * line_height + (count - 1.0) * leading;
    let width = shaped_lines
        .iter()
        .map(|shaped| shaped.width)
        .fold(0.0, f64::max);
    let across = |line_width: f64| match flags.horizontal {
        HAlign::Left => rect.left(),
        HAlign::Right => rect.right() - line_width,
        HAlign::HCenter => rect.left() + (rect.width - line_width) / 2.0,
    };
    let top = match flags.vertical {
        VAlign::Top => rect.top(),
        VAlign::Bottom => rect.bottom() - height,
        VAlign::VCenter => rect.top() + (rect.height - height) / 2.0,
    };
    let mut lines = Vec::with_capacity(shaped_lines.len());
    for (at, shaped) in shaped_lines.into_iter().enumerate() {
        let baseline = top + at as f64 * (line_height + leading) + ascent;
        lines.push((PointF::new(across(shaped.width), baseline), shaped));
    }
    Some(LaidOut {
        lines,
        bounds: RectF::new(across(width), top, width, height),
    })
}

// ---- the metrics ---------------------------------------------------------------------------------

/// QFontMetricsF for the fonts a canvas draws with, on a device of `dpi` dots an inch (a font
/// given in points is that many 72nds of an inch there). Every canvas hands out its own; a painter
/// with no canvas yet makes one at the device's resolution (SCREEN_DPI for an image or the
/// screen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    dpi: f64,
}

impl TextMetrics {
    pub const fn new(dpi: f64) -> Self {
        Self { dpi }
    }

    /// The metrics of the screen and of an image: 96 dots an inch.
    pub const fn screen() -> Self {
        Self::new(SCREEN_DPI)
    }

    pub fn dpi(&self) -> f64 {
        self.dpi
    }

    fn line_metrics(&self, font: &Font) -> (f64, f64, f64) {
        let pixels = font_pixels(font, self.dpi);
        if !pixels.is_finite() || pixels <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        with_engine(|engine| match engine.resolve(font) {
            Some(resolved) => engine.line_metrics(&resolved, pixels),
            None => (0.0, 0.0, 0.0),
        })
    }

    /// The width of each character of `text` as the shaped line gives it (a cluster's glyphs to
    /// the character that starts it), for eliding.
    fn character_widths(&self, font: &Font, text: &str) -> Vec<f64> {
        let starts: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
        let mut widths = vec![0.0; starts.len()];
        // Capitalising may change the characters (a sharp s to two capitals); then the line is
        // measured as given, so the glyphs still fall on the characters they draw.
        let keeps_characters = text.chars().all(|character| {
            let mut upper = character.to_uppercase();
            upper
                .next()
                .is_some_and(|first| first.len_utf8() == character.len_utf8())
                && upper.next().is_none()
        });
        let measured = if font.all_uppercase && !keeps_characters {
            Font {
                all_uppercase: false,
                ..font.clone()
            }
        } else {
            font.clone()
        };
        if let Some(shaped) = shape_line(&measured, self.dpi, text) {
            for glyph in &shaped.glyphs {
                if let Ok(at) = starts.binary_search(&glyph.range.start) {
                    widths[at] += glyph.advance;
                }
            }
        }
        widths
    }
}

impl FontMetrics for TextMetrics {
    fn ascent(&self, font: &Font) -> f64 {
        self.line_metrics(font).0
    }

    fn descent(&self, font: &Font) -> f64 {
        self.line_metrics(font).1
    }

    fn height(&self, font: &Font) -> f64 {
        let (ascent, descent, _) = self.line_metrics(font);
        ascent + descent
    }

    fn line_spacing(&self, font: &Font) -> f64 {
        let (ascent, descent, leading) = self.line_metrics(font);
        ascent + descent + leading
    }

    fn horizontal_advance(&self, font: &Font, text: &str) -> f64 {
        shape_line(font, self.dpi, &one_line(text)).map_or(0.0, |shaped| shaped.width)
    }

    /// The ink of the glyphs (their outlines' extent), with the origin on the baseline taken in,
    /// as QFontEngine::boundingBox starts its union at the origin.
    fn bounding_rect(&self, font: &Font, text: &str) -> RectF {
        let ink = text_path(font, self.dpi, PointF::default(), text).control_point_rect();
        let left = katana_math::cpp::min(0.0, ink.left());
        let top = katana_math::cpp::min(0.0, ink.top());
        let right = katana_math::cpp::max(0.0, ink.right());
        let bottom = katana_math::cpp::max(0.0, ink.bottom());
        RectF::new(left, top, right - left, bottom - top)
    }

    /// QFontMetricsF::elidedText (QTextEngine::elidedText): the text as it is when it fits or
    /// eliding is off; otherwise as many characters as fit beside the ellipsis - from the start,
    /// the end, or both ends taken alternately - and nothing when not even the ellipsis fits.
    fn elided_text(&self, font: &Font, text: &str, mode: Elide, width: f64) -> String {
        if mode == Elide::None || self.horizontal_advance(font, text) <= width {
            return String::from(text);
        }
        let available = width - self.horizontal_advance(font, ELLIPSIS);
        if available < 0.0 {
            return String::new();
        }
        let characters: Vec<char> = text.chars().collect();
        let widths = self.character_widths(font, text);
        let size = characters.len();
        let piece = |from: usize, to: usize| characters[from..to].iter().collect::<String>();
        match mode {
            Elide::Right => {
                let mut current = 0.0;
                let mut pos;
                let mut next = 0;
                loop {
                    pos = next;
                    next += 1;
                    current += widths[pos];
                    if next >= size || current >= available {
                        break;
                    }
                }
                format!("{}{ELLIPSIS}", piece(0, pos))
            }
            Elide::Left => {
                let mut current = 0.0;
                let mut pos;
                let mut next = size;
                loop {
                    pos = next;
                    next -= 1;
                    current += widths[next];
                    if next == 0 || current >= available {
                        break;
                    }
                }
                format!("{ELLIPSIS}{}", piece(pos, size))
            }
            Elide::Middle => {
                let mut left_width = 0.0;
                let mut right_width = 0.0;
                let mut left_pos;
                let mut right_pos;
                let mut next_left = 0;
                let mut next_right = size;
                loop {
                    left_pos = next_left;
                    right_pos = next_right;
                    next_left += 1;
                    left_width += widths[left_pos];
                    next_right -= 1;
                    right_width += widths[next_right];
                    if next_left >= size || next_right == 0 || left_width + right_width >= available
                    {
                        break;
                    }
                }
                format!("{}{ELLIPSIS}{}", piece(0, left_pos), piece(right_pos, size))
            }
            Elide::None => String::from(text),
        }
    }
}
