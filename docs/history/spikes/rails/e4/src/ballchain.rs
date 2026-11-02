//! The chain of flybys in ball arithmetic (puremp::Ball: a midpoint and a radius that is certain to hold the exact value), to see what a rigorous run costs and how many bits it
//! needs against the ordinary run: the same operations as `chain.rs`.

use std::time::Instant;

use puremp::int::Sign;
use puremp::{Ball, Int, Rational};

type B = Ball;
type V = [B; 3];

fn number(text: &str, p: u64) -> B {
    if let Some((numerator, denominator)) = text.split_once('/') {
        let ratio = Rational::new(Int::from_i128(numerator.parse().unwrap()), Int::from_i128(denominator.parse().unwrap()));
        return Ball::from_rational(&ratio, p);
    }
    if let Ok(whole) = text.parse::<i128>() {
        return Ball::from_int(&Int::from_i128(whole), p);
    }
    Ball::from_f64(text.parse().unwrap(), p)
}

fn dot(a: &V, b: &V) -> B {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn cross(a: &V, b: &V) -> V {
    [
        a[1].mul(&b[2]).sub(&a[2].mul(&b[1])),
        a[2].mul(&b[0]).sub(&a[0].mul(&b[2])),
        a[0].mul(&b[1]).sub(&a[1].mul(&b[0])),
    ]
}

fn add(a: &V, b: &V) -> V {
    [a[0].add(&b[0]), a[1].add(&b[1]), a[2].add(&b[2])]
}

fn sub(a: &V, b: &V) -> V {
    [a[0].sub(&b[0]), a[1].sub(&b[1]), a[2].sub(&b[2])]
}

fn scale(a: &V, k: &B) -> V {
    [a[0].mul(k), a[1].mul(k), a[2].mul(k)]
}

fn positive(b: &B) -> bool {
    !b.contains_zero() && b.midpoint().sign() == Sign::Positive
}

fn negative(b: &B) -> bool {
    !b.contains_zero() && b.midpoint().sign() == Sign::Negative
}

pub fn run(path: &str, p: u64) {
    let text = std::fs::read_to_string(path).expect("the chain file");
    let mut lines = text.lines();
    let start: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
    let lenses: Vec<Vec<&str>> = lines.map(|line| line.split_whitespace().collect()).collect();
    let began = Instant::now();
    let one = Ball::from_int(&Int::from_i128(1), p);
    let two = Ball::from_int(&Int::from_i128(2), p);
    let mut x: V = [number(start[1], p), number(start[2], p), number(start[3], p)];
    let mut v: V = [number(start[4], p), number(start[5], p), number(start[6], p)];
    let mut t = number(start[7], p);
    let mut periapsis = None;
    for (index, fields) in lenses.iter().enumerate() {
        let mu = number(fields[1], p);
        let rho = number(fields[2], p);
        let w: V = [number(fields[3], p), number(fields[4], p), number(fields[5], p)];
        let p0: V = [number(fields[6], p), number(fields[7], p), number(fields[8], p)];
        let u = sub(&v, &w);
        let q = sub(&x, &add(&p0, &scale(&w, &t)));
        let uu = dot(&u, &u);
        let qu = dot(&q, &u);
        let disc = qu.mul(&qu).sub(&uu.mul(&dot(&q, &q).sub(&rho.mul(&rho))));
        if !positive(&disc) || !negative(&qu) {
            println!("BALL lenses {} bits {p}: cannot decide at flyby {index} (the ball of the discriminant holds zero, or the ship may not go in)", lenses.len());
            return;
        }
        let s = qu.neg().sub(&disc.sqrt()).div(&uu);
        let r0 = add(&q, &scale(&u, &s));
        let energy = uu.div(&two).sub(&mu.div(&rho));
        if !positive(&energy) {
            println!("BALL lenses {} bits {p}: cannot decide at flyby {index} (bound?)", lenses.len());
            return;
        }
        let h = cross(&r0, &u);
        let k = sub(&scale(&cross(&u, &h), &one.div(&mu)), &scale(&r0, &one.div(&rho)));
        let e = dot(&k, &k).sqrt();
        let khat = scale(&k, &one.div(&e));
        let r1 = sub(&scale(&khat, &two.mul(&dot(&r0, &khat))), &r0);
        let v1 = sub(&u, &scale(&khat, &two.mul(&dot(&u, &khat))));
        let a = mu.div(&two.mul(&energy));
        let n = mu.div(&a.mul(&a).mul(&a)).sqrt();
        let argument = one.add(&rho.div(&a)).div(&e);
        // acosh(y) = ln(y + sqrt(y^2 - 1)); sinh(h) = (e^h - e^-h) / 2.
        let big_h = argument.add(&argument.mul(&argument).sub(&one).sqrt()).ln();
        let sinh = big_h.exp().sub(&big_h.neg().exp()).div(&two);
        let mean = e.mul(&sinh).sub(&big_h);
        t = t.add(&s).add(&two.mul(&mean).div(&n));
        x = add(&add(&p0, &scale(&w, &t)), &r1);
        v = add(&w, &v1);
        periapsis = Some(a.mul(&e.sub(&one)));
    }
    let milliseconds = began.elapsed().as_secs_f64() * 1e3;
    let result = periapsis.unwrap();
    println!(
        "BALL lenses {} bits {p}: {:.1} ms; the last periapsis {:.9e} m with a radius of {:.3e} m{}",
        lenses.len(),
        milliseconds,
        result.midpoint().to_f64(),
        result.radius().to_f64(),
        if result.radius().to_f64() < 1e-4 { "  <- under the tolerance" } else { "" }
    );
}
