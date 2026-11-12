use eframe::egui::*;
use std::time::{Duration, Instant};

const INK: Color32 = Color32::from_rgb(233, 237, 247);
const QUIET: Color32 = Color32::from_rgb(135, 146, 167);
const LILAC: Color32 = Color32::from_rgb(180, 158, 255);

pub fn background(ctx: &Context) {
    let rect = ctx.screen_rect();
    let p = ctx.layer_painter(LayerId::background());
    p.rect_filled(rect, 0.0, Color32::from_rgb(15, 18, 25));
}

pub fn patchboard(p: &Painter, rect: Rect) {
    p.rect_filled(rect, 14.0, Color32::from_rgb(18, 22, 30));
    let step = 22.0;
    let visible = rect.intersect(p.clip_rect());
    if !visible.is_positive() {
        return;
    }
    let x0 = (((visible.left() - rect.left() - 11.0) / step).ceil() as i32).max(0);
    let y0 = (((visible.top() - rect.top() - 11.0) / step).ceil() as i32).max(0);
    let x1 = ((visible.right() - rect.left() - 11.0) / step).floor() as i32;
    let y1 = ((visible.bottom() - rect.top() - 11.0) / step).floor() as i32;
    // Two triangles per dot, batched in one mesh; skip the offscreen canvas entirely.
    let mut dots = epaint::Mesh::default();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let center = rect.min + vec2(11.0 + x as f32 * step, 11.0 + y as f32 * step);
            dots.add_colored_rect(
                Rect::from_center_size(center, vec2(1.4, 1.4)),
                Color32::from_rgb(40, 47, 61),
            );
        }
    }
    p.add(dots);
}

pub fn apply_system_corners(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut std::ffi::c_void,
            attribute: u32,
            value: *const std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    if let Ok(handle) = cc.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            // Native DWM rounding on Windows 11, including native square corners when maximized.
            // Windows 10 ignores unsupported attribute 33 and retains its native square corners.
            unsafe {
                let rounded = 2u32;
                let dark = 1u32;
                DwmSetWindowAttribute(
                    handle.hwnd.get() as *mut _,
                    33,
                    &rounded as *const _ as *const _,
                    4,
                );
                DwmSetWindowAttribute(
                    handle.hwnd.get() as *mut _,
                    20,
                    &dark as *const _ as *const _,
                    4,
                );
            }
        }
    }
}

pub fn titlebar(ctx: &Context) {
    TopBottomPanel::top("window_titlebar")
        .exact_height(38.0)
        .frame(Frame::new().inner_margin(Margin::symmetric(12, 0)))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            let drag_rect = Rect::from_min_max(full.min, pos2(full.right() - 160.0, full.bottom()));
            let drag = ui.interact(drag_rect, Id::new("window_drag"), Sense::click_and_drag());
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if drag.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            // Plain window caption: no logo or brand mark occupies the routing toolbar.
            ui.painter().text(
                drag_rect.left_center() + vec2(12.0, 0.0),
                Align2::LEFT_CENTER,
                "Soon Link Pro",
                FontId::proportional(12.0),
                QUIET,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for index in 0..3 {
                    let (rect, response) = ui.allocate_exact_size(vec2(42.0, 30.0), Sense::click());
                    let close = index == 0;
                    if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            7.0,
                            if close {
                                Color32::from_rgb(153, 58, 81)
                            } else {
                                Color32::from_rgb(40, 46, 61)
                            },
                        );
                    }
                    let c = rect.center();
                    let stroke = Stroke::new(1.3, if response.hovered() { INK } else { QUIET });
                    match index {
                        0 => {
                            ui.painter()
                                .line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], stroke);
                            ui.painter()
                                .line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], stroke);
                        }
                        1 => {
                            if maximized {
                                ui.painter().rect_stroke(
                                    Rect::from_center_size(c + vec2(1.5, -1.5), vec2(8.0, 8.0)),
                                    1.0,
                                    stroke,
                                    StrokeKind::Inside,
                                );
                                ui.painter().rect_filled(
                                    Rect::from_center_size(c + vec2(-1.0, 1.0), vec2(8.0, 8.0)),
                                    1.0,
                                    Color32::from_rgb(20, 24, 33),
                                );
                            }
                            ui.painter().rect_stroke(
                                Rect::from_center_size(
                                    c + if maximized {
                                        vec2(-1.0, 1.0)
                                    } else {
                                        Vec2::ZERO
                                    },
                                    vec2(9.0, 9.0),
                                ),
                                1.0,
                                stroke,
                                StrokeKind::Inside,
                            );
                        }
                        _ => {
                            ui.painter()
                                .line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], stroke);
                        }
                    }
                    let response = response.on_hover_text(match index {
                        0 => "关闭窗口并收起到托盘",
                        1 => {
                            if maximized {
                                "还原"
                            } else {
                                "最大化"
                            }
                        }
                        _ => "最小化",
                    });
                    if response.clicked() {
                        ctx.send_viewport_cmd(match index {
                            0 => ViewportCommand::Close,
                            1 => ViewportCommand::Maximized(!maximized),
                            _ => ViewportCommand::Minimized(true),
                        });
                    }
                }
            });
        });
}

// Only the outer five logical pixels are reserved for native window resizing.
pub fn resize_edges(ctx: &Context) {
    if ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    }) {
        return;
    }
    let r = ctx.screen_rect();
    use ResizeDirection::*;
    let w = 5.0;
    let c = 12.0;
    let edges = [
        (
            Rect::from_min_max(r.min, r.min + vec2(c, c)),
            NorthWest,
            CursorIcon::ResizeNwSe,
        ),
        (
            Rect::from_min_max(pos2(r.right() - c, r.top()), pos2(r.right(), r.top() + c)),
            NorthEast,
            CursorIcon::ResizeNeSw,
        ),
        (
            Rect::from_min_max(
                pos2(r.left(), r.bottom() - c),
                pos2(r.left() + c, r.bottom()),
            ),
            SouthWest,
            CursorIcon::ResizeNeSw,
        ),
        (
            Rect::from_min_max(r.max - vec2(c, c), r.max),
            SouthEast,
            CursorIcon::ResizeNwSe,
        ),
        (
            Rect::from_min_max(r.min + vec2(c, 0.0), pos2(r.right() - c, r.top() + w)),
            North,
            CursorIcon::ResizeVertical,
        ),
        (
            Rect::from_min_max(pos2(r.left() + c, r.bottom() - w), r.max - vec2(c, 0.0)),
            South,
            CursorIcon::ResizeVertical,
        ),
        (
            Rect::from_min_max(r.min + vec2(0.0, c), pos2(r.left() + w, r.bottom() - c)),
            West,
            CursorIcon::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(pos2(r.right() - w, r.top() + c), r.max - vec2(0.0, c)),
            East,
            CursorIcon::ResizeHorizontal,
        ),
    ];
    for (index, (rect, direction, cursor)) in edges.into_iter().enumerate() {
        Area::new(Id::new(("window_resize_edge", index)))
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .order(Order::Foreground)
            .movable(false)
            .constrain(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (_, response) = ui.allocate_exact_size(rect.size(), Sense::drag());
                let response = response.on_hover_cursor(cursor);
                if response.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
    }
}

#[derive(Default)]
pub struct Toasts {
    messages: Vec<(String, Instant)>,
}
impl Toasts {
    pub fn push(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some((_, since)) = self.messages.iter_mut().find(|(text, _)| *text == message) {
            *since = Instant::now();
            return;
        }
        self.messages.push((message, Instant::now()));
        if self.messages.len() > 3 {
            self.messages.remove(0);
        }
    }
    pub fn show(&mut self, ctx: &Context) {
        self.messages
            .retain(|(_, time)| time.elapsed() < Duration::from_secs(5));
        if self.messages.is_empty() {
            return;
        }
        // This entire area is click-through and never takes focus.
        Area::new(Id::new("nonblocking_toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-25.0, -49.0))
            .order(Order::Tooltip)
            .interactable(false)
            .movable(false)
            .show(ctx, |ui| {
                ui.set_max_width(410.0);
                for (message, _) in &self.messages {
                    Frame::new()
                        .fill(Color32::from_rgb(37, 35, 54))
                        .corner_radius(12.0)
                        .inner_margin(Margin::symmetric(17, 13))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").size(10.0).color(LILAC));
                                ui.add(
                                    Label::new(RichText::new(message).size(13.0).color(INK)).wrap(),
                                );
                            });
                        });
                    ui.add_space(5.0);
                }
            });
    }
}

pub fn refine_style(style: &mut Style) {
    let visuals = &mut style.visuals;
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_fill = Color32::from_rgb(20, 24, 33);
    visuals.panel_fill = Color32::from_rgb(20, 24, 33);
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(48, 55, 70));
    visuals.window_shadow = epaint::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(7);
        widget.expansion = 0.0;
    }
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(31, 37, 49);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(31, 37, 49);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(51, 59, 75));
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 47, 62);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(40, 47, 62);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(83, 95, 119));
    visuals.widgets.active.bg_fill = Color32::from_rgb(47, 55, 71);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(47, 55, 71);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, LILAC.gamma_multiply(0.65));
    visuals.widgets.open.bg_fill = Color32::from_rgb(53, 45, 73);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(53, 45, 73);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, LILAC.gamma_multiply(0.4));
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.fg_stroke = Stroke::new(1.0, INK);
    }
    visuals.selection.bg_fill = Color32::from_rgb(68, 53, 100);
    visuals.selection.stroke = Stroke::new(1.0, INK);
    visuals.extreme_bg_color = Color32::from_rgb(24, 29, 39);
    style.spacing.interact_size = vec2(36.0, 36.0);
    style.spacing.button_padding = vec2(13.0, 7.0);
    style.spacing.item_spacing = vec2(10.0, 9.0);
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.combo_height = 280.0;
}

pub fn dialog<'a>(title: impl Into<WidgetText>) -> Window<'a> {
    Window::new(title).frame(
        Frame::new()
            .fill(Color32::from_rgb(20, 24, 33))
            .stroke(Stroke::new(1.0, Color32::from_rgb(48, 55, 70)))
            .corner_radius(14.0)
            .inner_margin(20.0)
            .shadow(epaint::Shadow {
                offset: [0, 12],
                blur: 32,
                spread: 0,
                color: Color32::from_black_alpha(90),
            }),
    )
}

pub fn section() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(27, 32, 43))
        .corner_radius(10.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(43, 50, 65)))
        .inner_margin(16.0)
}

pub fn intro(ui: &mut Ui, title: &str, description: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(20.0).strong().color(INK));
    ui.label(RichText::new(description).size(12.0).color(QUIET));
    ui.add_space(14.0);
}

fn curve_at(points: [Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    (points[0].to_vec2() * u * u * u
        + points[1].to_vec2() * 3.0 * u * u * t
        + points[2].to_vec2() * 3.0 * u * t * t
        + points[3].to_vec2() * t * t * t)
        .to_pos2()
}

fn smooth_step(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn cable_color(source: Color32, target: Color32, t: f32) -> Color32 {
    // Only the final quarter takes on the destination color.
    let mix = smooth_step((t - 0.72) / 0.28) * 0.72;
    Color32::from_rgb(
        lerp(source.r() as f32..=target.r() as f32, mix) as u8,
        lerp(source.g() as f32..=target.g() as f32, mix) as u8,
        lerp(source.b() as f32..=target.b() as f32, mix) as u8,
    )
}

// Bound the curve-to-chord error in physical pixels, including high-DPI screens.
fn curve_segments(points: [Pos2; 4], pixels_per_point: f32) -> usize {
    let a = (points[2] - points[1]) - (points[1] - points[0]);
    let b = (points[3] - points[2]) - (points[2] - points[1]);
    let acceleration = 6.0 * a.length().max(b.length());
    let tolerance = 0.20 / pixels_per_point;
    (acceleration / (8.0 * tolerance))
        .sqrt()
        .ceil()
        .clamp(16.0, 256.0) as usize
}

fn ribbon_sample(
    mesh: &mut epaint::Mesh,
    position: Pos2,
    normal: Vec2,
    width: f32,
    feather: f32,
    color: Color32,
    connect: bool,
) {
    // Custom meshes bypass egui's path feathering. Add a one-physical-pixel
    // alpha ramp on both edges so diagonal lines retain subpixel coverage.
    let inner = (width - feather).max(0.0) * 0.5;
    let outer = (width + feather) * 0.5;
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(position + normal * outer, Color32::TRANSPARENT);
    mesh.colored_vertex(position + normal * inner, color);
    mesh.colored_vertex(position - normal * inner, color);
    mesh.colored_vertex(position - normal * outer, Color32::TRANSPARENT);
    if connect {
        for edge in 0..3 {
            mesh.add_triangle(i - 4 + edge, i - 3 + edge, i + edge);
            mesh.add_triangle(i - 3 + edge, i + edge + 1, i + edge);
        }
    }
}

pub fn route_cable(
    painter: &Painter,
    points: [Pos2; 4],
    source: Color32,
    target: Color32,
    width: f32,
    opacity: f32,
) {
    let pixels_per_point = painter.ctx().pixels_per_point().max(0.1);
    let segments = curve_segments(points, pixels_per_point);
    let mut mesh = epaint::Mesh::default();
    mesh.vertices.reserve((segments + 1) * 4);
    mesh.indices.reserve(segments * 18);
    for segment in 0..=segments {
        let t = segment as f32 / segments as f32;
        let position = curve_at(points, t);
        let tangent =
            curve_at(points, (t + 0.001).min(1.0)) - curve_at(points, (t - 0.001).max(0.0));
        ribbon_sample(
            &mut mesh,
            position,
            tangent.normalized().rot90(),
            width,
            1.0 / pixels_per_point,
            cable_color(source, target, t).gamma_multiply(opacity),
            segment > 0,
        );
    }
    painter.add(mesh);
}

pub fn signal_beam(
    painter: &Painter,
    points: [Pos2; 4],
    phase: f32,
    source: Color32,
    target: Color32,
    strength: f32,
) {
    // Let the head travel beyond the destination so the tail drains into the port
    // before the cycle wraps. Fade the whole beam smoothly on arrival and departure.
    let head = phase * 1.20;
    let envelope = smooth_step(head / 0.06) * (1.0 - smooth_step((head - 0.88) / 0.30));
    let tail = (head - 0.16).max(0.0);
    let end = head.min(1.0);
    if tail >= end || envelope <= 0.0 {
        return;
    }
    let pixels_per_point = painter.ctx().pixels_per_point().max(0.1);
    let segments = ((curve_segments(points, pixels_per_point) as f32 * (end - tail)).ceil()
        as usize)
        .clamp(8, 64);
    let mut mesh = epaint::Mesh::default();
    mesh.vertices.reserve((segments + 1) * 4 * 3);
    mesh.indices.reserve(segments * 18 * 3);
    for (width, opacity) in [(7.0, 0.07), (3.8, 0.16), (1.9, 1.0)] {
        for segment in 0..=segments {
            let fraction = segment as f32 / segments as f32;
            let t = lerp(tail..=end, fraction);
            let position = curve_at(points, t);
            let tangent =
                curve_at(points, (t + 0.001).min(1.0)) - curve_at(points, (t - 0.001).max(0.0));
            let normal = tangent.normalized().rot90();
            let alpha = smooth_step((t - (head - 0.16)) / 0.16) * strength * opacity * envelope;
            let color = cable_color(source, target, t).gamma_multiply(alpha);
            ribbon_sample(
                &mut mesh,
                position,
                normal,
                width,
                1.0 / pixels_per_point,
                color,
                segment > 0,
            );
        }
    }
    painter.add(mesh);
    if head < 1.0 {
        let arrival = 1.0 - smooth_step((head - 0.90) / 0.10);
        painter.circle_filled(
            curve_at(points, head),
            2.0,
            Color32::WHITE.gamma_multiply(strength * envelope * arrival * 0.85),
        );
    }
}
