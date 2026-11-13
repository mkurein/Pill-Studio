//! Display formatting only.

pub fn hz(v: f64) -> String {
    if v >= 1e6 {
        let s = format!("{:.3}", v / 1e6);
        format!("{} МГц", s.trim_end_matches('0').trim_end_matches('.'))
    } else if v >= 1e3 {
        let s = format!("{:.3}", v / 1e3);
        format!("{} кГц", s.trim_end_matches('0').trim_end_matches('.'))
    } else {
        format!("{:.0} Гц", v)
    }
}

pub fn bytes(n: u32) -> String {
    if n >= 1024 && n.is_multiple_of(1024) {
        format!("{} КБ", n / 1024)
    } else if n >= 10 * 1024 {
        format!("{:.1} КБ", n as f64 / 1024.0)
    } else {
        format!("{} Б", n)
    }
}

pub fn uptime(ms: u32) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}ч {:02}м {:02}с", s / 3600, s / 60 % 60, s % 60)
    } else if s >= 60 {
        format!("{}м {:02}с", s / 60, s % 60)
    } else {
        format!("{}.{} с", s, ms % 1000 / 100)
    }
}

pub fn ago(now: f64, t: f64) -> String {
    let d = (now - t).max(0.0);
    if d < 1.0 {
        "только что".into()
    } else if d < 60.0 {
        format!("{:.0} с назад", d)
    } else {
        format!("{:.0} мин назад", d / 60.0)
    }
}

pub fn clock(t: f64) -> String {
    let ms = (t * 1000.0) as u64;
    format!("{:02}:{:02}.{:03}", ms / 60000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn bin_groups(v: u32, bits: u32) -> String {
    let mut s = String::new();
    for i in (0..bits).rev() {
        s.push(if v >> i & 1 == 1 { '1' } else { '0' });
        if i % 4 == 0 && i != 0 {
            s.push(' ');
        }
    }
    s
}
