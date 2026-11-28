use super::*;

#[test]
fn a_band_reaches_a_screen_above_and_below() {
    assert_eq!(band_around(5000, 1000, 20_000), Band { top: 4000, rows: 3000 });
}

#[test]
fn a_band_stays_within_the_page() {
    assert_eq!(band_around(0, 1000, 20_000), Band { top: 0, rows: 3000 });
    assert_eq!(band_around(19_500, 1000, 20_000), Band { top: 17_000, rows: 3000 });
    assert_eq!(band_around(0, 1000, 600), Band { top: 0, rows: 600 });
}

#[test]
fn a_band_is_never_taller_than_the_renderer_paints() {
    let band = band_around(30_000, 9000, 100_000);
    assert_eq!(band.rows, TALLEST_BAND);
}

#[test]
fn a_band_runs_out_half_a_screen_early() {
    let band = Band { top: 4000, rows: 3000 };
    assert!(!runs_out(band, 5000, 1000, 20_000));
    assert!(runs_out(band, 4400, 1000, 20_000));
    assert!(runs_out(band, 5600, 1000, 20_000));
}

#[test]
fn a_band_at_an_end_of_the_page_never_runs_out_there() {
    assert!(!runs_out(Band { top: 0, rows: 3000 }, 0, 1000, 20_000));
    assert!(!runs_out(Band { top: 17_000, rows: 3000 }, 19_000, 1000, 20_000));
}

fn kinds(asks: &[Ask]) -> Vec<&'static str> {
    asks.iter()
        .map(|ask| match ask {
            Ask::Open { .. } => "open",
            Ask::Resize { .. } => "resize",
            Ask::Paint { .. } => "paint",
            Ask::Hover { .. } => "hover",
            Ask::Press { .. } => "press",
            Ask::Drag { .. } => "drag",
            Ask::Click { .. } => "click",
            Ask::Copy => "copy",
            Ask::Close => "close",
        })
        .collect()
}

fn paint(top: u32) -> Ask {
    Ask::Paint { number: 1, band: Band { top, rows: 10 } }
}

#[test]
fn only_the_last_of_a_run_of_paints_and_hovers_is_done() {
    let batch = vec![paint(0), Ask::Hover { x: 1.0, y: 1.0 }, paint(10), Ask::Hover { x: 2.0, y: 2.0 }, paint(20)];
    let kept = latest(batch);
    assert_eq!(kinds(&kept), ["hover", "paint"]);
    assert!(matches!(kept.last(), Some(Ask::Paint { band: Band { top: 20, .. }, .. })));
}

#[test]
fn nothing_before_a_new_mail_is_done() {
    let batch = vec![
        paint(0),
        Ask::Copy,
        Ask::Open {
            number: 2,
            html: String::new(),
            sight: Sight { width: 1, height: 1, scale: 1.0 },
            band: Band::default(),
        },
        Ask::Hover { x: 0.0, y: 0.0 },
    ];
    assert_eq!(kinds(&latest(batch)), ["open", "hover"]);
}

#[test]
fn presses_clicks_and_copies_keep_their_order() {
    let band = Band::default();
    let batch = vec![
        Ask::Press { x: 1.0, y: 1.0, band },
        Ask::Drag { x: 2.0, y: 1.0, band },
        Ask::Drag { x: 3.0, y: 1.0, band },
        Ask::Copy,
        Ask::Click { x: 1.0, y: 1.0 },
    ];
    assert_eq!(kinds(&latest(batch)), ["press", "drag", "copy", "click"]);
}
