//! The Waterloo & City line's Working Timetable (polish spec §4): read from
//! `pdftotext -bbox` output of the owner's own copy of LU WTT No. 7, checked
//! against the figures the WTT itself publishes, and put into the Drain world
//! as its timetable. Only this code is in the repository: the PDF and anything
//! made from it stay outside it (`external/wtt/`, git-ignored).
//!
//! The WTT's train-service pages are tables, one column per trip, one row per
//! timing point. `pdftotext -bbox` gives every word with its box, so rows and
//! columns are found by position, never by counting spaces: fractions of a
//! minute are small words abutting the minutes (`12` = ½, `14` = ¼, `34` = ¾,
//! or a numerator and denominator stacked), and `23z57` is 23:57 with the
//! train-wash mark. Deterministic: same input, same output.

use std::collections::BTreeMap;

use signalbox_core::network::Dir;
use signalbox_core::sim::Sim;
use signalbox_core::time::fmt_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::{CallFile, EndFile, EntryFile, PositionFile, ServiceFile, WorldFile};

/// Row labels sit left of this (PDF points).
const LABEL_X: f64 = 100.0;
/// Table cells sit right of this.
const DATA_X: f64 = 128.0;
/// A word belongs to the row whose label is at most this far above or below.
const ROW_TOL: f64 = 4.0;
/// A cell belongs to the column whose train number is centred at most this far away.
const COL_TOL: f64 = 10.0;
/// Stacked fraction digits are shorter than this; every other word is taller.
const STACK_H: f64 = 4.0;
/// WTT times before this hour are after midnight (the line is shut 01:00–05:00).
const NIGHT_H: u32 = 4;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WttError {
    #[error("page {0}: {1}")]
    Format(usize, String),
    #[error("no Monday to Friday train service pages found")]
    NoPages,
    #[error("unknown day code `{0}` on train {1} trip {2}")]
    DayCode(String, u16, u16),
    #[error("check failed: {0}")]
    Check(String),
    #[error("cannot put into the world: {0}")]
    Apply(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    /// Bank → Waterloo.
    West,
    /// Waterloo → Bank.
    East,
}

/// One column of the train service pages. Times are seconds since the
/// midnight before the service day (so 00:30 is 24:30).
#[derive(Debug, Clone, PartialEq)]
pub struct Trip {
    pub train: u16,
    pub trip: u16,
    pub bound: Bound,
    /// Words in the notes rows: day codes, `Start`, `Ety`, `YW`, `Shed`, `Rd`.
    pub notes: Vec<String>,
    /// Bank platform.
    pub platform: Option<String>,
    pub bank: Option<u32>,
    /// Waterloo arrival (westbound: platform 26; eastbound: platform 25).
    pub arr: Option<u32>,
    /// `Pfm 25`/`Pfm 26` in the arrival row: the move starts standing in that platform.
    pub starts_in: Option<String>,
    pub dep: Option<u32>,
    pub siding: Option<u32>,
    pub depot: Option<u32>,
    /// A time with the train-wash mark `z`.
    pub wash: bool,
    /// The next trip's start; `None` for `Stop`.
    pub to_form: Option<u32>,
}

impl Trip {
    pub fn has(&self, note: &str) -> bool {
        self.notes.iter().any(|n| n == note)
    }

    fn times(&self) -> impl Iterator<Item = u32> + '_ {
        [self.bank, self.arr, self.dep, self.siding, self.depot].into_iter().flatten()
    }

    pub fn first(&self) -> u32 {
        self.times().min().unwrap_or(0)
    }

    pub fn last(&self) -> u32 {
        self.times().max().unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
struct Word {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    text: String,
}

fn attr(tag: &str, name: &str) -> Option<f64> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')? + at;
    tag[at..end].parse().ok()
}

fn decode(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The words of each page of `pdftotext -bbox` output.
fn pages(xhtml: &str) -> Vec<Vec<Word>> {
    let mut out = Vec::new();
    for page in xhtml.split("<page ").skip(1) {
        let mut words = Vec::new();
        let mut rest = page;
        while let Some(at) = rest.find("<word ") {
            rest = &rest[at..];
            let (Some(close), Some(end)) = (rest.find('>'), rest.find("</word>")) else { break };
            let tag = &rest[..close];
            if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (attr(tag, "xMin"), attr(tag, "yMin"), attr(tag, "xMax"), attr(tag, "yMax")) {
                words.push(Word { x0, y0, x1, y1, text: decode(&rest[close + 1..end]) });
            }
            rest = &rest[end..];
        }
        out.push(words);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Row {
    Train,
    Trip,
    Crew,
    Notes,
    Platform,
    Bank,
    Arr,
    Dep,
    Siding,
    Depot,
    ToForm,
    ByCrew,
}

/// Row labels (left of `LABEL_X`) by height.
fn anchors(words: &[Word]) -> Vec<(f64, Row)> {
    let mut out = Vec::new();
    for w in words.iter().filter(|w| w.x0 < LABEL_X) {
        let next = words
            .iter()
            .filter(|v| (v.y0 - w.y0).abs() < 0.5 && v.x0 > w.x1 && v.x0 < w.x1 + 5.0)
            .map(|v| v.text.as_str())
            .next();
        let row = match (w.text.as_str(), next) {
            ("Train", Some("No.")) => Row::Train,
            ("Trip", _) => Row::Trip,
            ("Crew", _) => Row::Crew,
            ("Notes", _) => Row::Notes,
            ("Platform", _) => Row::Platform,
            ("BANK", _) => Row::Bank,
            ("arr.", _) => Row::Arr,
            ("dep.", _) => Row::Dep,
            ("Waterloo", Some("Siding")) => Row::Siding,
            ("Waterloo", Some("Depot")) => Row::Depot,
            ("To", Some("form")) => Row::ToForm,
            ("By", _) => Row::ByCrew,
            _ => continue,
        };
        out.push((w.y0, row));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[derive(Debug, Clone)]
enum Tok {
    /// Hours, minutes (once read), seconds of fraction, wash mark.
    Time { h: u32, m: Option<u32>, frac: u32, wash: bool, x0: f64, x1: f64, y0: f64 },
    Text { text: String, x0: f64, x1: f64 },
}

impl Tok {
    fn centre(&self) -> f64 {
        match self {
            Tok::Time { x0, x1, .. } | Tok::Text { x0, x1, .. } => (x0 + x1) / 2.0,
        }
    }
}

fn two_digits(s: &str) -> Option<u32> {
    (s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// One row's words (sorted by x) as times and texts.
fn tokens(row: Row, words: &[&Word], stacked: &[&Word], page: usize) -> Result<Vec<Tok>, WttError> {
    let timed = matches!(row, Row::Bank | Row::Arr | Row::Dep | Row::Siding | Row::Depot | Row::ToForm);
    let mut out: Vec<Tok> = Vec::new();
    for w in words {
        let t = w.text.as_str();
        if let Some(Tok::Time { m, frac, x1, .. }) = out.last_mut() {
            if m.is_none() && (2.0..5.0).contains(&(w.x0 - *x1)) {
                if let Some(v) = two_digits(t) {
                    *m = Some(v);
                    *x1 = w.x1;
                    continue;
                }
            }
            if m.is_some() && *frac == 0 && (w.x0 - *x1).abs() < 0.4 {
                let f = match t {
                    "14" => Some(15),
                    "12" => Some(30),
                    "34" => Some(45),
                    _ => None,
                };
                if let Some(f) = f {
                    *frac = f;
                    *x1 = w.x1;
                    continue;
                }
            }
        }
        if timed {
            let b = t.as_bytes();
            if b.len() == 5 && b[2] == b'z' {
                if let (Some(h), Some(m)) = (two_digits(&t[..2]), two_digits(&t[3..])) {
                    out.push(Tok::Time { h, m: Some(m), frac: 0, wash: true, x0: w.x0, x1: w.x1, y0: w.y0 });
                    continue;
                }
            }
            let after_pfm = matches!(out.last(), Some(Tok::Text { text, .. }) if text == "Pfm");
            if let (Some(h), false) = (two_digits(t), after_pfm) {
                out.push(Tok::Time { h, m: None, frac: 0, wash: false, x0: w.x0, x1: w.x1, y0: w.y0 });
                continue;
            }
        }
        out.push(Tok::Text { text: w.text.clone(), x0: w.x0, x1: w.x1 });
    }
    for tok in &mut out {
        if let Tok::Time { h, m, frac, x1, y0, .. } = tok {
            if m.is_none() {
                return Err(WttError::Format(page, format!("hours {h:02} without minutes")));
            }
            let mut st: Vec<&&Word> =
                stacked.iter().filter(|s| (s.x0 - *x1).abs() < 0.6 && (-1.0..5.0).contains(&(s.y0 - *y0))).collect();
            if !st.is_empty() {
                st.sort_by(|a, b| a.y0.total_cmp(&b.y0));
                let digits: Vec<u32> = st.iter().filter_map(|s| s.text.parse().ok()).collect();
                *frac = match digits.as_slice() {
                    [1, 4] => 15,
                    [1, 2] => 30,
                    [3, 4] => 45,
                    _ => return Err(WttError::Format(page, format!("odd stacked fraction {digits:?}"))),
                };
            }
        }
    }
    Ok(out)
}

fn seconds(h: u32, m: u32, frac: u32) -> u32 {
    let h = if h < NIGHT_H { h + 24 } else { h };
    h * 3600 + m * 60 + frac
}

/// Every Monday-to-Friday trip, in page and column order.
pub fn parse(xhtml: &str) -> Result<Vec<Trip>, WttError> {
    let mut trips = Vec::new();
    for (pi, words) in pages(xhtml).into_iter().enumerate() {
        let page = pi + 1;
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        let mf = texts.windows(3).any(|w| w == ["MONDAYS", "TO", "FRIDAYS"]) && !texts.contains(&"SATURDAYS");
        let bound = match (texts.contains(&"WESTBOUND"), texts.contains(&"EASTBOUND")) {
            (true, false) => Bound::West,
            (false, true) => Bound::East,
            _ => continue,
        };
        if !mf {
            continue;
        }
        let words: Vec<Word> = words.into_iter().filter(|w| !w.text.chars().all(|c| c == '.')).collect();
        let an = anchors(&words);
        let starts: Vec<f64> = an.iter().filter(|a| a.1 == Row::Train).map(|a| a.0).collect();
        for (bi, &y0) in starts.iter().enumerate() {
            let y1 = starts.get(bi + 1).copied().unwrap_or(f64::INFINITY);
            let rows: Vec<(f64, Row)> = an.iter().copied().filter(|a| a.0 >= y0 - 1.0 && a.0 < y1 - 1.0).collect();
            let data: Vec<&Word> = words.iter().filter(|w| w.x0 >= DATA_X && w.y0 >= y0 - 1.0 && w.y0 < y1 - 1.0).collect();
            let mut cols: Vec<(f64, u16)> = Vec::new();
            for w in data.iter().filter(|w| (w.y0 - y0).abs() < 1.0) {
                let n = w.text.parse().map_err(|_| WttError::Format(page, format!("train number `{}`", w.text)))?;
                cols.push(((w.x0 + w.x1) / 2.0, n));
            }
            cols.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (stacked, plain): (Vec<&Word>, Vec<&Word>) = data
                .iter()
                .copied()
                .partition(|w| w.y1 - w.y0 < STACK_H && w.text.len() == 1 && w.text.as_bytes()[0].is_ascii_digit());
            // Each word to its row (or an unlabelled notes line).
            let mut by_row: BTreeMap<(Option<Row>, i64), Vec<&Word>> = BTreeMap::new();
            for w in plain {
                let near = rows.iter().min_by(|a, b| (a.0 - w.y0).abs().total_cmp(&(b.0 - w.y0).abs()));
                let key = match near {
                    Some(&(y, r)) if (y - w.y0).abs() <= ROW_TOL => (Some(r), 0),
                    _ => (None, w.y0.round() as i64),
                };
                by_row.entry(key).or_default().push(w);
            }
            let mut cells: Vec<BTreeMap<Row, Vec<Tok>>> = vec![BTreeMap::new(); cols.len()];
            let mut extra: Vec<Vec<String>> = vec![Vec::new(); cols.len()];
            for ((row, _), mut ws) in by_row {
                ws.sort_by(|a, b| a.x0.total_cmp(&b.x0));
                for tok in tokens(row.unwrap_or(Row::Notes), &ws, &stacked, page)? {
                    let c = cols
                        .iter()
                        .enumerate()
                        .min_by(|a, b| (a.1.0 - tok.centre()).abs().total_cmp(&(b.1.0 - tok.centre()).abs()))
                        .filter(|(_, c)| (c.0 - tok.centre()).abs() <= COL_TOL)
                        .map(|(i, _)| i)
                        .ok_or_else(|| WttError::Format(page, format!("a cell in no column: {tok:?}")))?;
                    match row {
                        Some(r) => cells[c].entry(r).or_default().push(tok),
                        None => {
                            if let Tok::Text { text, .. } = tok {
                                extra[c].push(text);
                            }
                        }
                    }
                }
            }
            for (c, (cell, extra)) in cells.into_iter().zip(extra).enumerate() {
                trips.push(trip(page, bound, cols[c].1, cell, extra)?);
            }
        }
    }
    if trips.is_empty() {
        return Err(WttError::NoPages);
    }
    Ok(trips)
}

fn trip(page: usize, bound: Bound, train: u16, mut cell: BTreeMap<Row, Vec<Tok>>, extra: Vec<String>) -> Result<Trip, WttError> {
    let err = |what: String| WttError::Format(page, format!("train {train}: {what}"));
    let mut take = |r: Row| cell.remove(&r).unwrap_or_default();
    let text = |toks: &[Tok]| -> Vec<String> {
        toks.iter().filter_map(|t| if let Tok::Text { text, .. } = t { Some(text.clone()) } else { None }).collect()
    };
    let mut wash = false;
    let mut time = |r: Row, toks: Vec<Tok>| -> Result<Option<u32>, WttError> {
        let ts: Vec<u32> = toks
            .iter()
            .filter_map(|t| match t {
                Tok::Time { h, m: Some(m), frac, wash: z, .. } => {
                    wash |= *z;
                    Some(seconds(*h, *m, *frac))
                }
                _ => None,
            })
            .collect();
        match ts.as_slice() {
            [] => Ok(None),
            [t] => Ok(Some(*t)),
            _ => Err(err(format!("{} times in row {r:?}", ts.len()))),
        }
    };
    let number = |toks: &[Tok], r: Row| -> Result<u16, WttError> {
        match text(toks).as_slice() {
            [n] => n.parse().map_err(|_| err(format!("{r:?} `{n}`"))),
            other => Err(err(format!("{r:?} {other:?}"))),
        }
    };
    let trip_no = number(&take(Row::Trip), Row::Trip)?;
    let mut notes = text(&take(Row::Notes));
    notes.extend(extra);
    let platform = text(&take(Row::Platform)).first().cloned();
    let arr_toks = take(Row::Arr);
    let starts_in = match text(&arr_toks).as_slice() {
        [] => None,
        [p] if p.starts_with("Pfm") => p.strip_prefix("Pfm").map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
        [p, n] if p == "Pfm" => Some(n.clone()),
        other => return Err(err(format!("arrival {other:?}"))),
    };
    let to_form_toks = take(Row::ToForm);
    let stop = text(&to_form_toks) == ["Stop"];
    let bank = time(Row::Bank, take(Row::Bank))?;
    let arr = time(Row::Arr, arr_toks)?;
    let dep = time(Row::Dep, take(Row::Dep))?;
    let siding = time(Row::Siding, take(Row::Siding))?;
    let depot = time(Row::Depot, take(Row::Depot))?;
    let to_form = time(Row::ToForm, to_form_toks)?;
    if stop == to_form.is_some() {
        return Err(err(format!("trip {trip_no}: `To form` must be a time or `Stop`")));
    }
    Ok(Trip { train, trip: trip_no, bound, notes, platform, bank, arr, starts_in, dep, siding, depot, wash, to_form })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Day {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
}

/// The weekday Drain runs (spec P15): Wednesday, the plain midweek day.
pub const DAY: Day = Day::Wed;

/// The days a note like `MO`, `TThX` or `MWO` names, and whether they are
/// the only days (`O`) or the excepted ones (`X`); `None` if it is not a day code.
fn day_code(note: &str) -> Option<(Vec<Day>, bool)> {
    let (body, only) = match note.as_bytes().last()? {
        b'O' => (&note[..note.len() - 1], true),
        b'X' => (&note[..note.len() - 1], false),
        _ => return None,
    };
    let mut days = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let (d, n) = if rest.starts_with("Th") {
            (Day::Thu, 2)
        } else {
            match rest.as_bytes()[0] {
                b'M' => (Day::Mon, 1),
                b'T' => (Day::Tue, 1),
                b'W' => (Day::Wed, 1),
                b'F' => (Day::Fri, 1),
                _ => return None,
            }
        };
        days.push(d);
        rest = &rest[n..];
    }
    (!days.is_empty()).then_some((days, only))
}

/// Notes that are neither day codes nor one of these are an error, so a
/// different WTT cannot slip an unknown restriction past the importer.
const PLAIN_NOTES: [&str; 6] = ["Start", "Ety", "YW", "Shed", "Rd", "RR"];

/// The trips that run on `day`.
pub fn on_day(trips: &[Trip], day: Day) -> Result<Vec<Trip>, WttError> {
    let mut out = Vec::new();
    for t in trips {
        let mut runs = true;
        for n in &t.notes {
            match day_code(n) {
                Some((days, only)) => runs &= days.contains(&day) == only,
                None if PLAIN_NOTES.contains(&n.as_str()) => {}
                None => return Err(WttError::DayCode(n.clone(), t.train, t.trip)),
            }
        }
        if runs {
            out.push(t.clone());
        }
    }
    Ok(out)
}

/// What the WTT says about itself, to check a parse against.
#[derive(Debug, Clone)]
pub struct Checks {
    /// Bank platform, bound, published running time in seconds (Waterloo ⇄ that platform).
    pub running: Vec<(&'static str, Bound, u32)>,
    /// Time, trains in service.
    pub snapshots: Vec<(u32, usize)>,
    /// From, to, mean interval between Bank departures (seconds).
    pub intervals: Vec<(u32, u32, u32)>,
}

const fn hm(h: u32, m: u32) -> u32 {
    h * 3600 + m * 60
}

impl Checks {
    /// WTT No. 7, page 2. The snapshot table prints 3 trains at 21:00, but its
    /// own workings (page 5: 201 finishes at 21:37) give 4, and so does every
    /// trip; the table is taken to predate the revision that lengthened the
    /// evening peak.
    pub fn waterloo_city() -> Checks {
        Checks {
            running: vec![("7", Bound::West, 210), ("8", Bound::West, 240), ("7", Bound::East, 255), ("8", Bound::East, 240)],
            snapshots: vec![
                (hm(6, 0), 1),
                (hm(9, 0), 5),
                (hm(12, 0), 3),
                (hm(15, 0), 3),
                (hm(18, 0), 5),
                (hm(21, 0), 4),
                (hm(24, 0), 2),
            ],
            intervals: vec![
                (hm(7, 30), hm(9, 30), 165),
                (hm(11, 0), hm(15, 30), 300),
                (hm(16, 30), hm(19, 45), 165),
                (hm(19, 45), hm(21, 30), 210),
                (hm(21, 30), hm(23, 30), 360),
                (hm(23, 30), hm(26, 0), 600),
            ],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckReport {
    pub trips: usize,
    pub trains: usize,
    /// Trips run in exactly the published time, and trips given longer.
    pub running_exact: usize,
    pub running_longer: usize,
    pub links: usize,
    pub snapshots: Vec<(u32, usize)>,
    /// From, to, mean interval in seconds (rounded).
    pub intervals: Vec<(u32, u32, u32)>,
}

/// Each train's trips in running order.
fn by_train(trips: &[Trip]) -> BTreeMap<u16, Vec<&Trip>> {
    let mut out: BTreeMap<u16, Vec<&Trip>> = BTreeMap::new();
    for t in trips {
        out.entry(t.train).or_default().push(t);
    }
    for v in out.values_mut() {
        v.sort_by_key(|t| (t.first(), t.trip));
    }
    out
}

fn running(t: &Trip) -> Option<u32> {
    match t.bound {
        Bound::West => Some(t.arr?.checked_sub(t.bank?)?),
        Bound::East => Some(t.bank?.checked_sub(t.dep?)?),
    }
}

/// Check one day's trips against the WTT's own figures (spec §4.3).
pub fn check(trips: &[Trip], c: &Checks) -> Result<CheckReport, WttError> {
    let mut r = CheckReport { trips: trips.len(), ..Default::default() };
    let fail = |s: String| Err(WttError::Check(s));
    for t in trips {
        let (Some(rt), Some(p)) = (running(t), t.platform.as_deref()) else { continue };
        let Some(&(_, _, want)) = c.running.iter().find(|x| x.0 == p && x.1 == t.bound) else {
            return fail(format!("train {} trip {}: no running time for platform {p}", t.train, t.trip));
        };
        match rt.cmp(&want) {
            std::cmp::Ordering::Less => {
                return fail(format!("train {} trip {} runs in {rt} s, under the published {want} s", t.train, t.trip));
            }
            std::cmp::Ordering::Equal => r.running_exact += 1,
            std::cmp::Ordering::Greater => r.running_longer += 1,
        }
    }
    let trains = by_train(trips);
    r.trains = trains.len();
    // Service periods: runs of trips linked by `To form`, with whether any carries passengers.
    let mut periods: Vec<(u32, u32, bool)> = Vec::new();
    for (n, ts) in &trains {
        let mut cur: Option<(u32, u32, bool)> = None;
        for (i, t) in ts.iter().enumerate() {
            let p = cur.get_or_insert((t.first(), t.last(), false));
            p.1 = t.last();
            p.2 |= !t.has("Ety");
            match (t.to_form, ts.get(i + 1)) {
                (Some(f), Some(next)) => {
                    if next.first() != f || next.bound == t.bound || t.last() > f {
                        return fail(format!("train {n} trip {} forms {} at {}, not trip {}", t.trip, fmt_hms(f.into()), fmt_hms(next.first().into()), next.trip));
                    }
                    r.links += 1;
                }
                (Some(f), None) => return fail(format!("train {n} trip {} forms a trip at {} that is not there", t.trip, fmt_hms(f.into()))),
                (None, next) => {
                    if let Some(next) = next {
                        if !next.has("Start") || next.first() < t.last() {
                            return fail(format!("train {n} trip {} stops but trip {} does not start", t.trip, next.trip));
                        }
                    }
                    periods.extend(cur.take());
                }
            }
        }
        periods.extend(cur);
    }
    for &(at, want) in &c.snapshots {
        let n = periods.iter().filter(|p| p.2 && p.0 <= at && at < p.1).count();
        r.snapshots.push((at, n));
        if n != want {
            return fail(format!("{} trains in service at {}, the WTT says {want}", n, fmt_hms(at.into())));
        }
    }
    let mut deps: Vec<u32> = trips.iter().filter(|t| t.bound == Bound::West).filter_map(|t| t.bank).collect();
    deps.sort();
    for &(from, to, want) in &c.intervals {
        let xs: Vec<u32> = deps.iter().copied().filter(|d| (from..=to).contains(d)).collect();
        if xs.len() < 2 {
            return fail(format!("fewer than two Bank departures {}–{}", fmt_hms(from.into()), fmt_hms(to.into())));
        }
        let mean = f64::from(xs[xs.len() - 1] - xs[0]) / (xs.len() - 1) as f64;
        r.intervals.push((from, to, mean.round() as u32));
        if (mean - f64::from(want)).abs() > 6.0 {
            return fail(format!("Bank departures {}–{} every {mean:.0} s, the WTT says {want} s", fmt_hms(from.into()), fmt_hms(to.into())));
        }
    }
    Ok(r)
}

/// Where the WTT's places are on Drain (spec §4.4).
pub const BANK: &str = "BNK";
pub const WATERLOO: &str = "WTL";
pub const ARRIVAL: &str = "26";
pub const DEPARTURE: &str = "25";
/// Waterloo roads 5, 6 and 7 behind the platforms: both the reversing siding and the depot.
pub const ROADS: (&str, [&str; 3]) = ("DPT", ["5", "6", "7"]);
/// Minimum time between one train leaving a road and the next arriving in it.
const ROAD_GAP_S: u32 = 60;
/// A train coming out of the depot appears this long before it leaves.
const APPEAR_S: u32 = 600;

/// The headcode of a trip (spec P18, amended): train number and trip
/// number, unique per service; the panel shows only the train number (the
/// service's display headcode).
pub fn headcode(t: &Trip) -> String {
    format!("{}/{}", t.train, t.trip)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplyReport {
    pub services: usize,
    pub entries: usize,
    /// Empty moves left out (train/trip).
    pub dropped_empty: Vec<String>,
    /// Trains left out because they only run empty.
    pub dropped_trains: Vec<u16>,
    /// Trains ended early to free a road for the night, and where they stable.
    pub shortened: Vec<String>,
    /// Stays in Waterloo roads, by road.
    pub road_use: Vec<(String, usize)>,
    pub start_time: String,
}

/// A stay in a Waterloo road: (train index in `chains`, trip index of the trip that arrives, or
/// `None` for a train that appears there), from, to (`None`: to the end of the day).
#[derive(Debug, Clone, Copy)]
struct Stay {
    chain: usize,
    arrives: Option<usize>,
    from: u32,
    to: Option<u32>,
}

/// Put one day's trips into `w` (Drain) in place of its timetable (spec §4.4).
pub fn apply(w: &mut WorldFile, day: &[Trip]) -> Result<ApplyReport, WttError> {
    let mut rep = ApplyReport::default();
    let fail = |s: String| WttError::Apply(s);
    let mut chains: Vec<Vec<Trip>> = Vec::new();
    for (n, ts) in by_train(day) {
        let kept: Vec<Trip> = ts.iter().filter(|t| !t.has("Ety")).map(|t| (*t).clone()).collect();
        rep.dropped_empty.extend(ts.iter().filter(|t| t.has("Ety")).map(|t| headcode(t)));
        if kept.is_empty() {
            rep.dropped_trains.push(n);
        } else {
            chains.push(kept);
        }
    }
    for ch in &chains {
        if let Some(w) = ch.windows(2).find(|w| w[0].bound == w[1].bound) {
            return Err(fail(format!("{} and {} run the same way one after the other", headcode(&w[0]), headcode(&w[1]))));
        }
        for t in ch {
            let ok = match t.bound {
                Bound::West => t.bank.is_some() && t.arr.is_some() && t.platform.is_some() && t.starts_in.is_none(),
                Bound::East => t.bank.is_some() && t.dep.is_some() && t.platform.is_some() && (t.siding.is_some() || t.depot.is_some()),
            };
            if !ok {
                return Err(fail(format!("trip {} is not a Bank–Waterloo run", headcode(t))));
            }
        }
    }
    let first_dep = chains.iter().map(|c| c[0].first()).min().ok_or_else(|| fail("no trips".into()))?;
    let start = (first_dep.saturating_sub(APPEAR_S)) / 300 * 300;
    // Allocate roads; a train whose last stay leaves no road for later ones ends at Bank instead.
    let roads = loop {
        let stays = stays(&chains, start);
        match allocate(&stays) {
            Ok(r) => break r.into_iter().zip(stays).collect::<Vec<_>>(),
            Err(blocked_at) => {
                let open: Vec<&Stay> = stays.iter().filter(|s| s.to.is_none() && s.from <= blocked_at).collect();
                let Some(&&last) = open.iter().max_by_key(|s| s.from) else {
                    return Err(fail(format!("no Waterloo road free at {}", fmt_hms(blocked_at.into()))));
                };
                rep.shortened.push(shorten(&mut chains, last.chain)?);
            }
        }
    };
    let mut road_of: BTreeMap<(usize, Option<usize>), &str> = BTreeMap::new();
    let mut use_count: BTreeMap<&str, usize> = BTreeMap::new();
    for (r, s) in &roads {
        road_of.insert((s.chain, s.arrives), ROADS.1[*r]);
        *use_count.entry(ROADS.1[*r]).or_default() += 1;
    }
    rep.road_use = use_count.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let fmt = |t: Option<u32>| t.map(|v| fmt_hms(v.into()));
    let call = |place: &str, pf: &str, arr: Option<u32>, dep: Option<u32>| CallFile {
        place: place.into(),
        platform: Some(pf.into()),
        arr: fmt(arr),
        dep: fmt(dep),
        stop: true,
    };
    let train_type = w.train_types.first().map(|t| t.code.clone()).ok_or_else(|| fail("no train type".into()))?;
    let mut services = Vec::new();
    let mut entries = Vec::new();
    for (ci, ch) in chains.iter().enumerate() {
        for (i, t) in ch.iter().enumerate() {
            let bank_pf = t.platform.as_deref().expect("checked above");
            let calls = match t.bound {
                Bound::West => {
                    let mut c = vec![call(BANK, bank_pf, None, t.bank), call(WATERLOO, ARRIVAL, t.arr, t.dep)];
                    if let Some(at) = t.siding.or(t.depot) {
                        let road = road_of.get(&(ci, Some(i))).ok_or_else(|| fail(format!("{}: no road", headcode(t))))?;
                        c.push(call(ROADS.0, road, Some(at), None));
                    } else {
                        // Its last call: run into platform 26 and stand at its starting
                        // signal. A booked stop there would wait for that signal to clear
                        // before the train could stable; a timed pass does not.
                        c[1].dep = None;
                        c[1].stop = false;
                    }
                    c
                }
                Bound::East => {
                    let road = match i {
                        0 => road_of.get(&(ci, None)),
                        _ => road_of.get(&(ci, Some(i - 1))),
                    }
                    .ok_or_else(|| fail(format!("{}: no road", headcode(t))))?;
                    vec![call(ROADS.0, road, None, t.siding.or(t.depot)), call(WATERLOO, DEPARTURE, t.arr, t.dep), call(BANK, bank_pf, t.bank, None)]
                }
            };
            let end = match ch.get(i + 1) {
                Some(n) => EndFile::Form { service: headcode(n) },
                None => EndFile::Stable,
            };
            services.push(ServiceFile {
                headcode: headcode(t),
                display: Some(t.train.to_string()),
                train_type: train_type.clone(),
                calls,
                end,
            });
        }
        let first = &services[services.len() - ch.len()];
        let (place, pf, time) = match ch[0].bound {
            Bound::West => (BANK, ch[0].platform.clone().unwrap_or_default(), start),
            Bound::East => {
                let road = road_of[&(ci, None)];
                (ROADS.0, road.to_string(), ch[0].first().saturating_sub(APPEAR_S).max(start))
            }
        };
        entries.push(EntryFile {
            service: first.headcode.clone(),
            boundary: None,
            at: Some(stand(w, place, &pf)?),
            time: fmt_hms(time.into()),
            speed_kmh: 0.0,
            on_demand: false,
        });
    }
    entries.sort_by(|a, b| a.time.cmp(&b.time).then(a.service.cmp(&b.service)));
    rep.services = services.len();
    rep.entries = entries.len();
    rep.start_time = fmt_hms(start.into());
    w.services = services;
    w.entries = entries;
    w.options.start_time = rep.start_time.clone();
    w.options.min_dwell_s = [20, 30];
    World::from_file(w.clone()).map_err(|e| fail(format!("the world no longer loads: {e}")))?;
    Ok(rep)
}

/// Every stay in a Waterloo road, in time order.
fn stays(chains: &[Vec<Trip>], start: u32) -> Vec<Stay> {
    let mut out = Vec::new();
    for (ci, ch) in chains.iter().enumerate() {
        if ch[0].bound == Bound::East {
            let leave = ch[0].first();
            out.push(Stay { chain: ci, arrives: None, from: leave.saturating_sub(APPEAR_S).max(start), to: Some(leave) });
        }
        for (i, t) in ch.iter().enumerate() {
            if let (Bound::West, Some(at)) = (t.bound, t.siding.or(t.depot)) {
                out.push(Stay { chain: ci, arrives: Some(i), from: at, to: ch.get(i + 1).map(|n| n.first()) });
            }
        }
    }
    out.sort_by_key(|s| (s.from, s.chain));
    out
}

/// The road (index into `ROADS`) for each stay: the one free longest. `Err`
/// is the time a stay found none.
fn allocate(stays: &[Stay]) -> Result<Vec<usize>, u32> {
    let mut free_from: [Option<u32>; 3] = [Some(0); 3];
    let mut out = Vec::new();
    for s in stays {
        // The road free longest, so a late train is least likely to find its road still taken.
        let r = (0..3).filter(|&r| free_from[r].is_some_and(|f| f <= s.from)).min_by_key(|&r| (free_from[r], r)).ok_or(s.from)?;
        free_from[r] = s.to.map(|t| t + ROAD_GAP_S);
        out.push(r);
    }
    Ok(out)
}

/// End chain `ci` at its last Bank arrival instead, in a Bank platform no
/// later trip uses, so its last road stay is no longer needed.
fn shorten(chains: &mut [Vec<Trip>], ci: usize) -> Result<String, WttError> {
    let ch = &chains[ci];
    let Some(k) = ch.iter().rposition(|t| t.bound == Bound::East) else {
        return Err(WttError::Apply(format!("train {} never reaches Bank", ch[0].train)));
    };
    let arrive = ch[k].bank.unwrap_or(0);
    let used_later = |pf: &str| {
        chains.iter().enumerate().filter(|(i, _)| *i != ci).flat_map(|(_, c)| c).any(|t| t.platform.as_deref() == Some(pf) && t.bank.is_some_and(|b| b >= arrive))
    };
    let pf = ["7", "8"].into_iter().find(|p| !used_later(p)).ok_or_else(|| {
        WttError::Apply(format!("train {}: no Bank platform free from {}", chains[ci][0].train, fmt_hms(arrive.into())))
    })?;
    let ch = &mut chains[ci];
    ch.truncate(k + 1);
    ch[k].platform = Some(pf.to_string());
    ch[k].to_form = None;
    Ok(format!("{} stables at Bank platform {pf} at {}", headcode(&ch[k]), fmt_hms(arrive.into())))
}

/// A train standing in `place` platform `pf`, its head 1 m short of the end
/// that faces the platform's starting signal.
fn stand(w: &WorldFile, place: &str, pf: &str) -> Result<PositionFile, WttError> {
    let p = w
        .platforms
        .iter()
        .find(|p| p.place == place && p.platform == pf)
        .ok_or_else(|| WttError::Apply(format!("Drain has no platform {place} {pf}")))?;
    let mut probe = w.clone();
    probe.services.clear();
    probe.entries.clear();
    let world = World::from_file(probe).map_err(|e| WttError::Apply(e.to_string()))?;
    let sim = Sim::new(world, 0);
    let net = &sim.world().net;
    let seg = net.segments.iter().position(|s| s.name == p.segment).expect("platform segments exist");
    let sg = &net.segments[seg];
    let mut found = Vec::new();
    for (dir, offset) in [(Dir::Up, p.to_m - 1.0), (Dir::Down, p.from_m + 1.0)] {
        let id = signalbox_core::ids::SegmentId::from_idx(seg);
        if let Some((_, d)) = net.first_signal_ahead(id, dir, sg.along(offset, dir), 30.0, sim.points()) {
            found.push((d, dir, offset));
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let &(_, dir, offset_m) = found.first().ok_or_else(|| WttError::Apply(format!("no starting signal for {place} {pf}")))?;
    Ok(PositionFile { segment: p.segment.clone(), offset_m, direction: dir })
}
