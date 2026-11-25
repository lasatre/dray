//! Dray's components, painted from `theme`: buttons, status pills, D-ended progress bars,
//! sidebar items, key hints, list-row backgrounds and the keyboard focus ring. They only
//! draw and report clicks; all behavior stays in `app.rs`.

use crate::icons::Icon;
use crate::theme::{self, Palette};
use eframe::egui::{
    self, Color32, Id, Painter, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, TextWrapMode,
    Ui, Vec2, Widget, WidgetText, pos2, vec2,
};

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a.lerp_to_gamma(b, t)
    }
}

/// Hover/press feedback eases in over 100 ms.
fn hover_t(ui: &Ui, resp: &Response) -> f32 {
    let hot = resp.hovered() || resp.is_pointer_button_down_on();
    ui.ctx().animate_bool_with_time(resp.id.with("hover"), hot, 0.1)
}

// ------------------------------------------------------------------------------- Button

#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    /// One per view: brand fill, on-brand text.
    Primary,
    /// surface-raised with a border-strong edge.
    Secondary,
    /// No frame until hovered (toolbars, inline actions).
    Ghost,
    /// danger text and edge.
    Danger,
}

/// A 28 px button (`brand/reference/components/Button`).
pub struct Button {
    variant: Variant,
    text: WidgetText,
    icon: Option<Icon>,
    icon_size: f32,
    selected: bool,
}

impl Button {
    pub fn new(variant: Variant, text: impl Into<WidgetText>) -> Self {
        Button { variant, text: text.into(), icon: None, icon_size: theme::ICON, selected: false }
    }
    pub fn primary(text: impl Into<WidgetText>) -> Self {
        Self::new(Variant::Primary, text)
    }
    pub fn secondary(text: impl Into<WidgetText>) -> Self {
        Self::new(Variant::Secondary, text)
    }
    pub fn ghost(text: impl Into<WidgetText>) -> Self {
        Self::new(Variant::Ghost, text)
    }
    pub fn danger(text: impl Into<WidgetText>) -> Self {
        Self::new(Variant::Danger, text)
    }
    /// Icon before the label (or alone, for an empty label).
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Icon-only toolbar button with a 20 px icon.
    pub fn toolbar(icon: Icon) -> Self {
        Button { icon_size: theme::ICON_TOOLBAR, ..Self::ghost("").icon(icon) }
    }
    /// Toggle buttons: selected state uses the selection tint.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for Button {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = theme::of(ui);
        let galley = self.text.into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Button);
        let pad = theme::SPACE_3;
        let has_text = !galley.is_empty();
        let icon_w = match (self.icon, has_text) {
            (Some(_), true) => self.icon_size + theme::SPACE_1,
            (Some(_), false) => self.icon_size,
            (None, _) => 0.0,
        };
        let width = if has_text { galley.size().x + icon_w + 2.0 * pad } else { theme::CONTROL_H }.max(theme::CONTROL_H);
        let label = if has_text { galley.text().to_string() } else { self.icon.map(|i| format!("{i:?}")).unwrap_or_default() };
        let (rect, resp) = ui.allocate_exact_size(vec2(width, theme::CONTROL_H), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &label));
        if ui.is_rect_visible(rect) {
            let t = hover_t(ui, &resp);
            let (fill, stroke, text) = match self.variant {
                Variant::Primary => (lerp(p.brand, p.brand_strong, t), Stroke::NONE, p.on_brand),
                Variant::Secondary => {
                    let rest = if self.selected { p.brand_subtle } else { p.surface_raised };
                    (lerp(rest, p.surface_sunken, t), Stroke::new(1.0, p.border_strong), p.ink)
                }
                Variant::Ghost => {
                    let rest = if self.selected { p.brand_subtle } else { Color32::TRANSPARENT };
                    (lerp(rest, p.surface_sunken, t), Stroke::NONE, p.ink)
                }
                Variant::Danger => (lerp(p.surface_raised, p.surface_sunken, t), Stroke::new(1.0, p.danger), p.danger),
            };
            ui.painter().rect(rect, theme::radius(theme::RADIUS_MD), fill, stroke, StrokeKind::Inside);
            let content_w = galley.size().x + icon_w;
            let mut x = rect.center().x - content_w / 2.0;
            if let Some(icon) = self.icon {
                // Icons are ink-muted at rest and ink when hot or selected; the folder icon is
                // always brand; on a brand fill they take the text color.
                let tint = match self.variant {
                    Variant::Primary | Variant::Danger => text,
                    _ if icon == Icon::Folder => p.brand,
                    _ if self.selected => p.ink,
                    _ => lerp(p.ink_muted, p.ink, t),
                };
                let r = Rect::from_min_size(pos2(x, rect.center().y - self.icon_size / 2.0), Vec2::splat(self.icon_size));
                icon.image(self.icon_size, tint).paint_at(ui, r);
                x += icon_w;
            }
            let pos = pos2(x, rect.center().y - galley.size().y / 2.0);
            ui.painter().galley(pos, galley, text);
        }
        resp
    }
}

// --------------------------------------------------------------------------- Status pill

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Synced,
    Syncing,
    Offline,
    Failed,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Synced => "Synced",
            Status::Syncing => "Syncing",
            Status::Offline => "Offline",
            Status::Failed => "Failed",
        }
    }
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Status::Synced => p.success,
            Status::Syncing => p.info,
            Status::Offline => p.warning,
            Status::Failed => p.danger,
        }
    }
}

/// Round-capped polyline in a 12-unit icon box (the Status component's own glyphs).
fn glyph_line(painter: &Painter, origin: Pos2, scale: f32, pts: &[(f32, f32)], stroke: Stroke) {
    let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| origin + vec2(x, y) * scale).collect();
    painter.add(egui::Shape::line(pts.clone(), stroke));
    for p in pts {
        painter.circle_filled(p, stroke.width / 2.0, stroke.color);
    }
}

/// Arc around `(cx, cy)` with radius `r`, from angle `a0` to `a1` (radians, y down).
fn glyph_arc(painter: &Painter, origin: Pos2, scale: f32, (cx, cy, r): (f32, f32, f32), (a0, a1): (f32, f32), stroke: Stroke) {
    let n = 16;
    let pts: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    glyph_line(painter, origin, scale, &pts, stroke);
}

/// The 12 px status icons from `brand/reference/components/Status/preview.html`.
pub fn paint_status_icon(painter: &Painter, rect: Rect, status: Status, color: Color32) {
    use std::f32::consts::PI;
    let scale = rect.width() / 12.0;
    let o = rect.min;
    let s = Stroke::new(1.75 * scale, color);
    match status {
        Status::Synced => glyph_line(painter, o, scale, &[(2.0, 6.5), (4.5, 9.0), (10.0, 3.5)], s),
        Status::Syncing => {
            // M10 6 a4 4 0 1 1 -1.2 -2.9, then the arrow head M10 2 v2.5 H7.5
            glyph_arc(painter, o, scale, (6.0, 6.0, 4.0), (0.0, 2.0 * PI - 0.81), s);
            glyph_line(painter, o, scale, &[(10.0, 2.0), (10.0, 4.5), (7.5, 4.5)], s);
        }
        Status::Offline => {
            // M2 8.5 a5 5 0 0 1 8 0 (r = 5, chord 8: centre 3 below), M6 10.5 dot, M1.5 1.5 l9 9
            glyph_arc(painter, o, scale, (6.0, 11.5, 5.0), (PI + 0.6435, 2.0 * PI - 0.6435), s);
            painter.circle_filled(o + vec2(6.0, 10.5) * scale, s.width / 2.0, color);
            glyph_line(painter, o, scale, &[(1.5, 1.5), (10.5, 10.5)], s);
        }
        Status::Failed => {
            glyph_line(painter, o, scale, &[(3.0, 3.0), (9.0, 9.0)], s);
            glyph_line(painter, o, scale, &[(9.0, 3.0), (3.0, 9.0)], s);
        }
    }
}

/// Status pill (`brand/reference/components/Status`): icon plus word in the state color, on
/// surface-raised with a 1 px edge, square left corner and fully rounded right end.
pub fn status_pill(ui: &mut Ui, status: Status) -> Response {
    let p = theme::of(ui);
    let color = status.color(p);
    let font = theme::semibold_font(theme::FONT_UI_SMALL);
    let galley = ui.painter().layout_no_wrap(status.word().to_string(), font, color);
    let icon = 12.0;
    let size = vec2(theme::SPACE_2 + icon + theme::SPACE_1 + galley.size().x + theme::SPACE_3, theme::PILL_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(rect, theme::d_shape(theme::RADIUS_SM), p.surface_raised, Stroke::new(1.0, color), StrokeKind::Inside);
        let icon_rect = Rect::from_min_size(pos2(rect.left() + theme::SPACE_2, rect.center().y - icon / 2.0), vec2(icon, icon));
        paint_status_icon(painter, icon_rect, status, color);
        let text_pos = pos2(icon_rect.right() + theme::SPACE_1, rect.center().y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, color);
    }
    resp
}

// -------------------------------------------------------------------------- Progress bar

/// Determinate progress: a 6 px D-ended bar (square left, round right) in patina on a
/// `border` track. Only for progress tied to real bytes.
pub fn progress_bar(ui: &mut Ui, frac: f32, width: f32) -> Response {
    let p = theme::of(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, theme::d_shape(0.0), p.border);
        let frac = frac.clamp(0.0, 1.0);
        if frac > 0.0 {
            let fill = Rect::from_min_size(rect.min, vec2((rect.width() * frac).max(rect.height()), rect.height()));
            painter.rect_filled(fill, theme::d_shape(0.0), p.patina);
        }
    }
    resp
}

/// Progress bar plus percentage in mono.
pub fn progress(ui: &mut Ui, frac: f32, width: f32) {
    ui.spacing_mut().item_spacing.x = theme::SPACE_2;
    progress_bar(ui, frac, width);
    ui.label(mono_small(format!("{:.0}%", frac.clamp(0.0, 1.0) * 100.0)).color(theme::of(ui).ink_muted));
}

// ------------------------------------------------------------------------------ Text

/// `mono-small`: dates, sizes, numbers in dense columns and the status bar.
pub fn mono_small(text: impl Into<String>) -> RichText {
    RichText::new(text).text_style(TextStyle::Name(theme::MONO_SMALL.into()))
}

/// `ui-small` at weight 600 in ink-muted: section labels in panes and dialogs.
pub fn section_label(ui: &mut Ui, text: &str) -> Response {
    let p = theme::of(ui);
    ui.label(RichText::new(text).font(theme::semibold_font(theme::FONT_UI_SMALL)).color(p.ink_muted))
}

/// Keyboard hint (`radius-sm` tag, mono).
pub fn key_hint(ui: &mut Ui, keys: &str) -> Response {
    let p = theme::of(ui);
    egui::Frame::new()
        .fill(p.surface_sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(theme::radius(theme::RADIUS_SM))
        .inner_margin(egui::Margin::symmetric(theme::SPACE_1 as i8, 1))
        .show(ui, |ui| ui.label(mono_small(keys).color(p.ink)))
        .response
}

// -------------------------------------------------------------------------- Nav item

/// Sidebar entry: full-width `radius-md` row. Hover is a lighter well on the sunken
/// sidebar, selection is brand-subtle with ink text and no accent bar.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool, drop_target: bool) -> Response {
    let p = theme::of(ui);
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, theme::ROW_COMPACT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), selected, label));
    if ui.is_rect_visible(rect) {
        let t = hover_t(ui, &resp);
        let rest = if selected || drop_target { p.brand_subtle } else { Color32::TRANSPARENT };
        let fill = if selected || drop_target { rest } else { lerp(rest, p.surface, t) };
        let stroke = if drop_target { Stroke::new(2.0, p.brand) } else { Stroke::NONE };
        ui.painter().rect(rect, theme::radius(theme::RADIUS_MD), fill, stroke, StrokeKind::Inside);
        let icon_rect = Rect::from_min_size(pos2(rect.left() + theme::SPACE_2, rect.center().y - theme::ICON / 2.0), Vec2::splat(theme::ICON));
        let tint = if selected { p.ink } else { p.ink_muted };
        icon.image(theme::ICON, tint).paint_at(ui, icon_rect);
        let text_left = icon_rect.right() + theme::SPACE_2;
        let font = theme::ui_font(theme::FONT_UI_ROW);
        let galley = ui.painter().layout(label.to_string(), font, p.ink, rect.right() - text_left - theme::SPACE_2);
        let pos = pos2(text_left, rect.center().y - galley.size().y / 2.0);
        ui.painter().with_clip_rect(rect).galley(pos, galley, p.ink);
    }
    resp
}

// ---------------------------------------------------------------------------- List rows

/// Background of a file-list row: hover `surface-sunken`, selection `brand-subtle`, a drop
/// target also gets a 2 px brand edge; every row ends in a `border` hairline.
pub fn paint_row(painter: &Painter, rect: Rect, selected: bool, hovered: bool, drop_target: bool) {
    let p = if painter.ctx().global_style().visuals.dark_mode { &theme::DARK } else { &theme::LIGHT };
    let fill = if selected || drop_target {
        p.brand_subtle
    } else if hovered {
        p.surface_sunken
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.border));
    if drop_target {
        painter.rect_stroke(rect.shrink(1.0), theme::radius(theme::RADIUS_MD), Stroke::new(2.0, p.brand), StrokeKind::Inside);
    }
}

// ----------------------------------------------------------------------------- Focus ring

/// Draw the 2 px `focus-ring`, 2 px outside the focused control. Shown for keyboard focus
/// (Tab) and always for text inputs, like CSS `:focus-visible`.
pub fn focus_ring(ctx: &egui::Context, keyboard: bool) {
    let Some(id) = ctx.memory(|m| m.focused()) else { return };
    let Some(resp) = ctx.read_response(id) else { return };
    let is_text = egui::text_edit::TextEditState::load(ctx, id).is_some();
    if !keyboard && !is_text {
        return;
    }
    let p = theme::of_ctx(ctx);
    let w = theme::FOCUS_RING_WIDTH;
    let rect = resp.rect.expand(theme::FOCUS_RING_OFFSET + w / 2.0);
    let painter = ctx.layer_painter(egui::LayerId::new(resp.layer_id.order, Id::unique("dray-focus-ring")));
    painter.rect_stroke(
        rect,
        theme::radius(theme::RADIUS_MD + theme::FOCUS_RING_OFFSET + w / 2.0),
        Stroke::new(w, p.focus_ring),
        StrokeKind::Middle,
    );
}
