//! Calibração visual: a mesma sequência de 21 passos do `calibrate`, um passo por tela,
//! com o controle desenhado e um anel mostrando o tempo até pular.

use crate::backend::Ev;
use crate::labels;
use crate::pad::PadView;
use crate::window::App;
use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

const STEP_TIMEOUT: f64 = 10.0;

struct Inner {
    app: Weak<App>,
    id: String,
    dialog: adw::Dialog,
    stack: gtk::Stack,
    step: gtk::Label,
    title: gtk::Label,
    feedback: gtk::Label,
    countdown: gtk::Label,
    ring: gtk::DrawingArea,
    pad: PadView,
    started: Cell<Option<Instant>>,
    running: Cell<bool>,
    done_title: gtk::Label,
    done_text: gtk::Label,
    skip: gtk::Button,
    back: gtk::Button,
}

#[derive(Clone)]
pub struct CalDialog(Rc<Inner>);

pub fn open(app: &Rc<App>, id: &str) {
    let Some(b) = app.backend() else { return };
    if app.calibration.borrow().is_some() {
        return;
    }
    let dlg = CalDialog::new(app, id);
    *app.calibration.borrow_mut() = Some(dlg.clone());
    dlg.0.dialog.present(Some(&app.window));
    let (app, id) = (app.clone(), id.to_owned());
    glib::spawn_future_local(async move {
        if let Err(e) = b.start_calibration(&id, STEP_TIMEOUT).await {
            dlg.finish(false, &e);
            app.toast(&e);
        }
    });
}

impl CalDialog {
    fn new(app: &Rc<App>, id: &str) -> CalDialog {
        let step = gtk::Label::builder()
            .label("Preparando…")
            .css_classes(["dim-label", "caption-heading"])
            .build();
        let pad = PadView::new(300, 190);
        let title = gtk::Label::builder()
            .label(" ")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["big-instruction"])
            .build();
        let hint = gtk::Label::builder()
            .label("Segure até o controle acender. Solte para continuar.")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .build();
        let feedback = gtk::Label::builder()
            .label(" ")
            .css_classes(["success", "monospace", "caption"])
            .build();
        let ring = gtk::DrawingArea::builder()
            .content_width(40)
            .content_height(40)
            .build();
        let countdown = gtk::Label::builder()
            .label(" ")
            .css_classes(["dim-label", "caption"])
            .build();
        let timer = gtk::Box::builder()
            .spacing(10)
            .halign(gtk::Align::Center)
            .build();
        timer.append(&ring);
        timer.append(&countdown);
        let back = gtk::Button::builder()
            .label("Voltar")
            .css_classes(["flat"])
            .halign(gtk::Align::Center)
            .build();

        let running = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(18)
            .margin_start(24)
            .margin_end(24)
            .build();
        for w in [
            step.upcast_ref::<gtk::Widget>(),
            pad.area.upcast_ref(),
            title.upcast_ref(),
            hint.upcast_ref(),
            feedback.upcast_ref(),
            timer.upcast_ref(),
            back.upcast_ref(),
        ] {
            running.append(w);
        }

        let done_title = gtk::Label::builder().css_classes(["title-2"]).build();
        let done_text = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .build();
        let close = gtk::Button::builder()
            .label("Fechar")
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        let done = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .valign(gtk::Align::Center)
            .margin_start(24)
            .margin_end(24)
            .margin_bottom(24)
            .build();
        done.append(&done_title);
        done.append(&done_text);
        done.append(&close);

        let stack = gtk::Stack::new();
        stack.add_named(&running, Some("running"));
        stack.add_named(&done, Some("done"));

        let cancel = gtk::Button::builder().label("Cancelar").build();
        let skip = gtk::Button::builder().label("Pular").build();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&cancel);
        header.pack_end(&skip);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&stack));

        let dialog = adw::Dialog::builder()
            .title("Calibrar controle")
            .content_width(460)
            .child(&view)
            .build();

        let d = CalDialog(Rc::new(Inner {
            app: Rc::downgrade(app),
            id: id.to_owned(),
            dialog,
            stack,
            step,
            title,
            feedback,
            countdown,
            ring,
            pad,
            started: Cell::new(None),
            running: Cell::new(true),
            done_title,
            done_text,
            skip,
            back,
        }));

        let w = Rc::downgrade(&d.0);
        d.0.ring.set_draw_func(move |_, cr, wd, ht| {
            let Some(i) = w.upgrade() else { return };
            let frac = i
                .started
                .get()
                .map(|s| 1.0 - (s.elapsed().as_secs_f64() / STEP_TIMEOUT).clamp(0.0, 1.0))
                .unwrap_or(1.0);
            let (cx, cy) = (f64::from(wd) / 2.0, f64::from(ht) / 2.0);
            let r = cx.min(cy) - 3.0;
            cr.set_line_width(4.0);
            let dark = adw::StyleManager::default().is_dark();
            if dark {
                cr.set_source_rgb(0.2, 0.21, 0.24);
            } else {
                cr.set_source_rgb(0.89, 0.9, 0.93);
            }
            cr.arc(cx, cy, r, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            if dark {
                cr.set_source_rgb(0.54, 0.58, 1.0);
            } else {
                cr.set_source_rgb(0.29, 0.34, 0.89);
            }
            cr.arc(cx, cy, r, -PI / 2.0, -PI / 2.0 + 2.0 * PI * frac);
            let _ = cr.stroke();
        });
        let w = Rc::downgrade(&d.0);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(i) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if let Some(s) = i.started.get() {
                let left = (STEP_TIMEOUT - s.elapsed().as_secs_f64()).max(0.0).ceil();
                i.countdown
                    .set_text(&format!("{left:.0} s · pula sozinho se nada chegar"));
                i.ring.queue_draw();
            }
            glib::ControlFlow::Continue
        });

        let c = d.clone();
        cancel.connect_clicked(move |_| {
            c.control("cancel");
            c.0.dialog.close();
        });
        let c = d.clone();
        d.0.skip.connect_clicked(move |_| c.control("skip"));
        let c = d.clone();
        d.0.back.connect_clicked(move |_| c.control("back"));
        let c = d.clone();
        close.connect_clicked(move |_| {
            c.0.dialog.close();
        });
        let c = d.clone();
        d.0.dialog.connect_closed(move |_| {
            if c.0.running.get() {
                c.control("cancel");
            }
            if let Some(app) = c.0.app.upgrade() {
                *app.calibration.borrow_mut() = None;
                app.clone().refresh();
            }
        });
        d
    }

    fn control(&self, action: &str) {
        let Some(app) = self.0.app.upgrade() else {
            return;
        };
        let Some(b) = app.backend() else { return };
        let (id, action) = (self.0.id.clone(), action.to_owned());
        glib::spawn_future_local(async move {
            let _ = b.calibration_control(&id, &action).await;
        });
    }

    fn finish(&self, saved: bool, message: &str) {
        self.0.running.set(false);
        self.0.started.set(None);
        self.0.pad.set_prompt(None);
        self.0.skip.set_visible(false);
        self.0.done_title.set_text(if saved {
            "Pronto!"
        } else {
            "Calibração interrompida"
        });
        self.0.done_text.set_text(message);
        self.0.stack.set_visible_child_name("done");
    }

    pub fn on_event(&self, ev: &Ev) {
        let i = &self.0;
        match ev {
            Ev::Step {
                id,
                index,
                total,
                target,
            } if *id == i.id => {
                i.step.set_text(&format!("Passo {} de {total}", index + 1));
                i.title.set_text(&labels::instruction(*target));
                i.pad.set_prompt(Some(*target));
                i.started.set(Some(Instant::now()));
                i.back.set_sensitive(*index > 0);
                i.feedback.set_text(" ");
            }
            Ev::Hit {
                id,
                description,
                clash,
            } if *id == i.id => {
                i.started.set(None);
                i.pad.set_prompt(None);
                if clash.is_empty() {
                    i.feedback
                        .set_css_classes(&["success", "monospace", "caption"]);
                    i.feedback.set_text(&format!("OK · {description}"));
                } else {
                    i.feedback
                        .set_css_classes(&["warning", "monospace", "caption"]);
                    i.feedback
                        .set_text(&format!("{description} · igual a {}", clash.join(", ")));
                }
                i.countdown.set_text("Solte para continuar");
            }
            Ev::Skipped { id } if *id == i.id => {
                i.feedback.set_css_classes(&["dim-label", "caption"]);
                i.feedback.set_text("Pulado");
            }
            Ev::Finished { id, saved, message } if *id == i.id => self.finish(*saved, message),
            _ => {}
        }
    }
}
