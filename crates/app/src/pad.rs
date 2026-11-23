//! Desenho do controle, com as peças que acendem (o "controle é a interface" do design).

use gamesir_core::api::LiveState;
use gamesir_core::Target;
use gtk::cairo;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;
use std::rc::Rc;

#[derive(Default)]
struct State {
    active: BTreeSet<Target>,
    axes: BTreeMap<Target, i32>,
    /// Peça que o usuário deve apertar agora (calibração/captura): pisca em índigo.
    prompt: Option<Target>,
    blink: bool,
}

#[derive(Clone)]
pub struct PadView {
    pub area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
}

struct Palette {
    body: (f64, f64, f64),
    part: (f64, f64, f64),
    line: (f64, f64, f64),
    accent: (f64, f64, f64),
    accent_fg: (f64, f64, f64),
    muted: (f64, f64, f64),
}

fn hex(c: u32) -> (f64, f64, f64) {
    (
        f64::from((c >> 16) & 0xff) / 255.0,
        f64::from((c >> 8) & 0xff) / 255.0,
        f64::from(c & 0xff) / 255.0,
    )
}

fn palette() -> Palette {
    if adw::StyleManager::default().is_dark() {
        Palette {
            body: hex(0x24262b),
            part: hex(0x33353c),
            line: hex(0x44474f),
            accent: hex(0x8a95ff),
            accent_fg: hex(0x101330),
            muted: hex(0x9a9fac),
        }
    } else {
        Palette {
            body: hex(0xffffff),
            part: hex(0xe4e6ec),
            line: hex(0xd0d3dc),
            accent: hex(0x4b57e3),
            accent_fg: hex(0xffffff),
            muted: hex(0x656b7a),
        }
    }
}

fn set(cr: &cairo::Context, c: (f64, f64, f64)) {
    cr.set_source_rgb(c.0, c.1, c.2);
}

fn rrect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

impl PadView {
    pub fn new(width: i32, height: i32) -> PadView {
        let area = gtk::DrawingArea::builder()
            .content_width(width)
            .content_height(height)
            .hexpand(true)
            .accessible_role(gtk::AccessibleRole::Img)
            .build();
        area.update_property(&[gtk::accessible::Property::Label("Desenho do controle")]);
        let state = Rc::new(RefCell::new(State::default()));
        let s = state.clone();
        area.set_draw_func(move |_, cr, w, h| draw(cr, f64::from(w), f64::from(h), &s.borrow()));
        let v = PadView { area, state };
        // Pisca a peça pedida.
        let weak = v.area.downgrade();
        let s = v.state.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let Some(area) = weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            let mut st = s.borrow_mut();
            if st.prompt.is_some() {
                st.blink = !st.blink;
                area.queue_draw();
            }
            gtk::glib::ControlFlow::Continue
        });
        let a = v.area.clone();
        adw::StyleManager::default().connect_dark_notify(move |_| a.queue_draw());
        v
    }

    pub fn set_live(&self, live: &LiveState) {
        let mut s = self.state.borrow_mut();
        s.active = live.active.iter().copied().collect();
        s.axes = live.axes.clone();
        drop(s);
        self.area.queue_draw();
    }

    pub fn set_prompt(&self, t: Option<Target>) {
        let mut s = self.state.borrow_mut();
        s.prompt = t;
        s.blink = true;
        drop(s);
        self.area.queue_draw();
    }
}

fn draw(cr: &cairo::Context, w: f64, h: f64, s: &State) {
    // Desenho em 320 x 210, centralizado e escalado.
    let scale = (w / 320.0).min(h / 210.0);
    cr.translate((w - 320.0 * scale) / 2.0, (h - 210.0 * scale) / 2.0);
    cr.scale(scale, scale);
    let p = palette();
    let on = |t: Target| s.active.contains(&t) || (s.prompt == Some(t) && s.blink);
    let fill = |t: Target| if on(t) { p.accent } else { p.part };
    cr.set_line_width(1.2);

    // Gatilhos: preenchem conforme o valor.
    for (t, x) in [(Target::Lt, 52.0), (Target::Rt, 228.0)] {
        rrect(cr, x, 6.0, 40.0, 22.0, 7.0);
        set(cr, p.part);
        let _ = cr.fill_preserve();
        set(cr, p.line);
        let _ = cr.stroke();
        let v = f64::from(*s.axes.get(&t).unwrap_or(&0)) / 255.0;
        let v = if s.prompt == Some(t) && s.blink {
            1.0
        } else {
            v
        };
        if v > 0.0 {
            rrect(cr, x, 6.0 + 22.0 * (1.0 - v), 40.0, 22.0 * v, 7.0);
            set(cr, p.accent);
            let _ = cr.fill();
        }
    }
    // Ombros.
    for (t, x) in [(Target::Lb, 46.0), (Target::Rb, 218.0)] {
        rrect(cr, x, 30.0, 56.0, 14.0, 7.0);
        set(cr, fill(t));
        let _ = cr.fill();
    }

    // Corpo.
    cr.new_path();
    cr.move_to(80.0, 46.0);
    cr.line_to(240.0, 46.0);
    cr.curve_to(282.0, 46.0, 300.0, 80.0, 310.0, 140.0);
    cr.curve_to(318.0, 186.0, 296.0, 204.0, 274.0, 204.0);
    cr.curve_to(254.0, 204.0, 244.0, 192.0, 232.0, 172.0);
    cr.line_to(222.0, 160.0);
    cr.line_to(98.0, 160.0);
    cr.line_to(88.0, 172.0);
    cr.curve_to(76.0, 192.0, 66.0, 204.0, 46.0, 204.0);
    cr.curve_to(24.0, 204.0, 2.0, 186.0, 10.0, 140.0);
    cr.curve_to(20.0, 80.0, 38.0, 46.0, 80.0, 46.0);
    cr.close_path();
    set(cr, p.body);
    let _ = cr.fill_preserve();
    set(cr, p.line);
    let _ = cr.stroke();

    // Analógicos: a tampa anda conforme o eixo.
    let stick = |cx: f64, cy: f64, tx: Target, ty: Target, click: Target| {
        let ax = f64::from(*s.axes.get(&tx).unwrap_or(&0)) / 32767.0;
        let ay = f64::from(*s.axes.get(&ty).unwrap_or(&0)) / 32767.0;
        let blink_axis = s.blink && (s.prompt == Some(tx) || s.prompt == Some(ty));
        let (ax, ay) = match s.prompt {
            Some(t) if blink_axis && t == tx => (1.0, 0.0),
            Some(t) if blink_axis && t == ty => (0.0, 1.0),
            _ => (ax, ay),
        };
        cr.arc(cx, cy, 24.0, 0.0, 2.0 * PI);
        set(cr, p.part);
        let _ = cr.fill();
        let moved = ax.abs() > 0.25 || ay.abs() > 0.25;
        cr.arc(cx + ax * 9.0, cy + ay * 9.0, 15.0, 0.0, 2.0 * PI);
        set(cr, if on(click) || moved { p.accent } else { p.body });
        let _ = cr.fill_preserve();
        set(cr, p.line);
        let _ = cr.stroke();
    };
    stick(84.0, 96.0, Target::Lx, Target::Ly, Target::Ls);
    stick(200.0, 140.0, Target::Rx, Target::Ry, Target::Rs);

    // D-pad.
    let (dx, dy) = (120.0, 140.0);
    for (t, x, y, ww, hh) in [
        (Target::DpadUp, dx - 7.0, dy - 22.0, 14.0, 16.0),
        (Target::DpadDown, dx - 7.0, dy + 6.0, 14.0, 16.0),
        (Target::DpadLeft, dx - 22.0, dy - 7.0, 16.0, 14.0),
        (Target::DpadRight, dx + 6.0, dy - 7.0, 16.0, 14.0),
    ] {
        rrect(cr, x, y, ww, hh, 3.0);
        set(cr, fill(t));
        let _ = cr.fill();
    }
    rrect(cr, dx - 7.0, dy - 7.0, 14.0, 14.0, 2.0);
    set(cr, p.part);
    let _ = cr.fill();

    // ABXY.
    let (bx, by) = (240.0, 96.0);
    for (t, x, y, label) in [
        (Target::Y, bx, by - 18.0, "Y"),
        (Target::X, bx - 18.0, by, "X"),
        (Target::B, bx + 18.0, by, "B"),
        (Target::A, bx, by + 18.0, "A"),
    ] {
        cr.arc(x, y, 10.0, 0.0, 2.0 * PI);
        set(cr, fill(t));
        let _ = cr.fill();
        set(cr, if on(t) { p.accent_fg } else { p.muted });
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(10.0);
        if let Ok(ext) = cr.text_extents(label) {
            cr.move_to(
                x - ext.width() / 2.0 - ext.x_bearing(),
                y + ext.height() / 2.0,
            );
            let _ = cr.show_text(label);
        }
    }

    // Back, Start, Home.
    for (t, x) in [(Target::Back, 130.0), (Target::Start, 178.0)] {
        rrect(cr, x - 8.0, 92.0, 16.0, 9.0, 4.5);
        set(cr, fill(t));
        let _ = cr.fill();
    }
    cr.arc(154.0, 72.0, 11.0, 0.0, 2.0 * PI);
    set(cr, fill(Target::Home));
    let _ = cr.fill();
}
