//! The chain of flybys of `e4_chain.py`, run in a library: the same operations in the same order.

use std::time::Instant;

use crate::libs::Lib;

struct Lens<N> {
    mu: N,
    rho: N,
    w: [N; 3],
    p0: [N; 3],
}

fn number<L: Lib>(c: &mut L::Ctx, text: &str) -> L::N {
    // A whole number, a fraction of whole numbers (the grid of positions: a power of two for a denominator, so exact), or a float as Python writes it.
    if let Some((numerator, denominator)) = text.split_once('/') {
        let n = L::from_i128(c, numerator.parse().unwrap());
        let d = L::from_i128(c, denominator.parse().unwrap());
        return L::div(c, &n, &d);
    }
    if let Ok(whole) = text.parse::<i128>() {
        return L::from_i128(c, whole);
    }
    L::from_f64(c, text.parse().unwrap())
}

fn dot<L: Lib>(c: &mut L::Ctx, a: &[L::N; 3], b: &[L::N; 3]) -> L::N {
    let x = L::mul(c, &a[0], &b[0]);
    let y = L::mul(c, &a[1], &b[1]);
    let z = L::mul(c, &a[2], &b[2]);
    let xy = L::add(c, &x, &y);
    L::add(c, &xy, &z)
}

fn cross<L: Lib>(c: &mut L::Ctx, a: &[L::N; 3], b: &[L::N; 3]) -> [L::N; 3] {
    let mut part = |i: usize, j: usize| {
        let first = L::mul(c, &a[i], &b[j]);
        let second = L::mul(c, &a[j], &b[i]);
        L::sub(c, &first, &second)
    };
    [part(1, 2), part(2, 0), part(0, 1)]
}

fn add<L: Lib>(c: &mut L::Ctx, a: &[L::N; 3], b: &[L::N; 3]) -> [L::N; 3] {
    [L::add(c, &a[0], &b[0]), L::add(c, &a[1], &b[1]), L::add(c, &a[2], &b[2])]
}

fn sub<L: Lib>(c: &mut L::Ctx, a: &[L::N; 3], b: &[L::N; 3]) -> [L::N; 3] {
    [L::sub(c, &a[0], &b[0]), L::sub(c, &a[1], &b[1]), L::sub(c, &a[2], &b[2])]
}

fn scale<L: Lib>(c: &mut L::Ctx, a: &[L::N; 3], k: &L::N) -> [L::N; 3] {
    [L::mul(c, &a[0], k), L::mul(c, &a[1], k), L::mul(c, &a[2], k)]
}

/// Through the sphere of one lens: (the place and the time at which the ship leaves it, its velocity then, the periapsis), or why it does not go through.
fn flyby<L: Lib>(
    c: &mut L::Ctx,
    lens: &Lens<L::N>,
    x0: &[L::N; 3],
    t0: &L::N,
    v: &[L::N; 3],
) -> Result<([L::N; 3], L::N, [L::N; 3], L::N), &'static str> {
    let zero = L::from_i128(c, 0);
    let one = L::from_i128(c, 1);
    let two = L::from_i128(c, 2);
    let u = sub::<L>(c, v, &lens.w);
    let moved = scale::<L>(c, &lens.w, t0);
    let lens_now = add::<L>(c, &lens.p0, &moved);
    let q = sub::<L>(c, x0, &lens_now);
    let uu = dot::<L>(c, &u, &u);
    let qu = dot::<L>(c, &q, &u);
    let qq = dot::<L>(c, &q, &q);
    let rho2 = L::mul(c, &lens.rho, &lens.rho);
    let outside = L::sub(c, &qq, &rho2);
    let qu2 = L::mul(c, &qu, &qu);
    let uuo = L::mul(c, &uu, &outside);
    let disc = L::sub(c, &qu2, &uuo);
    if !(L::to_f64(&disc) > 0.0) || L::to_f64(&qu) >= 0.0 {
        return Err("misses the sphere");
    }
    let root = L::sqrt(c, &disc);
    let minus_qu = L::neg(c, &qu);
    let numerator = L::sub(c, &minus_qu, &root);
    let s = L::div(c, &numerator, &uu);
    let travelled = scale::<L>(c, &u, &s);
    let r0 = add::<L>(c, &q, &travelled);
    let half_uu = L::div(c, &uu, &two);
    let potential = L::div(c, &lens.mu, &lens.rho);
    let energy = L::sub(c, &half_uu, &potential);
    if L::to_f64(&energy) <= 0.0 {
        return Err("bound");
    }
    let h = cross::<L>(c, &r0, &u);
    let inverse_mu = L::div(c, &one, &lens.mu);
    let inverse_rho = L::div(c, &one, &lens.rho);
    let u_cross_h = cross::<L>(c, &u, &h);
    let first = scale::<L>(c, &u_cross_h, &inverse_mu);
    let second = scale::<L>(c, &r0, &inverse_rho);
    let k = sub::<L>(c, &first, &second);
    let kk = dot::<L>(c, &k, &k);
    let e = L::sqrt(c, &kk);
    let inverse_e = L::div(c, &one, &e);
    let khat = scale::<L>(c, &k, &inverse_e);
    let r_along = dot::<L>(c, &r0, &khat);
    let twice_r = L::mul(c, &two, &r_along);
    let reflected = scale::<L>(c, &khat, &twice_r);
    let r1 = sub::<L>(c, &reflected, &r0);
    let u_along = dot::<L>(c, &u, &khat);
    let twice_u = L::mul(c, &two, &u_along);
    let along = scale::<L>(c, &khat, &twice_u);
    let v1 = sub::<L>(c, &u, &along);
    let two_energy = L::mul(c, &two, &energy);
    let a = L::div(c, &lens.mu, &two_energy);
    let a2 = L::mul(c, &a, &a);
    let a3 = L::mul(c, &a2, &a);
    let ratio = L::div(c, &lens.mu, &a3);
    let n = L::sqrt(c, &ratio);
    let rho_over_a = L::div(c, &lens.rho, &a);
    let one_plus = L::add(c, &one, &rho_over_a);
    let argument = L::div(c, &one_plus, &e);
    let big_h = L::acosh(c, &argument);
    let sinh = L::sinh(c, &big_h);
    let e_sinh = L::mul(c, &e, &sinh);
    let mean = L::sub(c, &e_sinh, &big_h);
    let twice_mean = L::mul(c, &two, &mean);
    let flight = L::div(c, &twice_mean, &n);
    let t_in = L::add(c, t0, &s);
    let t1 = L::add(c, &t_in, &flight);
    let moved = scale::<L>(c, &lens.w, &t1);
    let lens_then = add::<L>(c, &lens.p0, &moved);
    let place = add::<L>(c, &lens_then, &r1);
    let velocity = add::<L>(c, &lens.w, &v1);
    let e_minus_one = L::sub(c, &e, &one);
    let periapsis = L::mul(c, &a, &e_minus_one);
    let _ = zero;
    Ok((place, t1, velocity, periapsis))
}

pub fn run<L: Lib>(path: &str, precision: usize) {
    let text = std::fs::read_to_string(path).expect("the chain file");
    let mut lines = text.lines();
    let start: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    let mut lenses: Vec<Vec<String>> = Vec::new();
    for line in lines {
        lenses.push(line.split_whitespace().map(str::to_owned).collect());
    }
    let mut best = f64::INFINITY;
    let mut last = None;
    let mut all = Vec::new();
    for repeat in 0..5 {
        let begin = Instant::now();
        let mut c = L::ctx(precision);
        let x = [number::<L>(&mut c, start[1]), number::<L>(&mut c, start[2]), number::<L>(&mut c, start[3])];
        let v = [number::<L>(&mut c, start[4]), number::<L>(&mut c, start[5]), number::<L>(&mut c, start[6])];
        let t = number::<L>(&mut c, start[7]);
        let parsed: Vec<Lens<L::N>> = lenses
            .iter()
            .map(|fields| Lens {
                mu: number::<L>(&mut c, &fields[1]),
                rho: number::<L>(&mut c, &fields[2]),
                w: [number::<L>(&mut c, &fields[3]), number::<L>(&mut c, &fields[4]), number::<L>(&mut c, &fields[5])],
                p0: [number::<L>(&mut c, &fields[6]), number::<L>(&mut c, &fields[7]), number::<L>(&mut c, &fields[8])],
            })
            .collect();
        let (mut x, mut t, mut v) = (x, t, v);
        let mut periapsides = Vec::new();
        let mut failure = None;
        for lens in &parsed {
            match flyby::<L>(&mut c, lens, &x, &t, &v) {
                Ok((place, time, velocity, periapsis)) => {
                    x = place;
                    t = time;
                    v = velocity;
                    periapsides.push(periapsis);
                }
                Err(reason) => {
                    failure = Some(reason);
                    break;
                }
            }
        }
        let milliseconds = begin.elapsed().as_secs_f64() * 1e3;
        best = best.min(milliseconds);
        if repeat == 0 {
            all = periapsides.iter().map(|p| L::canon(p)).collect();
        }
        last = Some((periapsides.last().map(|p| L::canon(p)), failure));
        if milliseconds > 3000.0 {
            break;
        }
    }
    if std::env::var("E4_TRACE").is_ok() {
        for (index, p) in all.iter().enumerate() {
            println!("STEP {index} {}{:x}*2^{}", if p.negative { "-" } else { "" }, p.mantissa, p.exponent);
        }
    }
    let (final_periapsis, failure) = last.unwrap();
    match (final_periapsis, failure) {
        (Some(p), None) => println!(
            "RESULT {} lenses {} bits {} best_ms {:.3} periapsis {}{:x}*2^{}",
            L::NAME,
            lenses.len(),
            precision,
            best,
            if p.negative { "-" } else { "" },
            p.mantissa,
            p.exponent
        ),
        (_, Some(reason)) => println!("RESULT {} lenses {} bits {} BROKEN: {reason} after {} flybys", L::NAME, lenses.len(), precision, all.len()),
        (None, None) => println!("RESULT {} no lenses", L::NAME),
    }
}
