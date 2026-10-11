// provenance: original
//! The numeral grammar of `klein.science.expr.v1`.
//!
//! Every check is a hand-written byte scan over ASCII digits. Only the spelling is checked here: a rational need not
//! be in lowest terms at this layer (D-20).

/// `[1-9][0-9]*`
fn is_positive(s: &str) -> bool {
    match s.as_bytes() {
        [first, rest @ ..] => (b'1'..=b'9').contains(first) && rest.iter().all(u8::is_ascii_digit),
        [] => false,
    }
}

/// An Int numeral: `0|-?[1-9][0-9]*`.
pub fn is_int(s: &str) -> bool {
    s == "0" || is_positive(s.strip_prefix('-').unwrap_or(s))
}

/// The numerator `p` of a Rat is a non-zero Int numeral.
pub(crate) fn is_rat_numerator(p: &str) -> bool {
    p != "0" && is_int(p)
}

/// The denominator `q` of a Rat matches `[1-9][0-9]*` and is not `1`.
pub(crate) fn is_rat_denominator(q: &str) -> bool {
    q != "1" && is_positive(q)
}

/// A Rat numeral pair: `p` is a non-zero Int and `q` matches `[1-9][0-9]*` and is not `1`. No gcd check (D-20).
pub fn is_rat(p: &str, q: &str) -> bool {
    is_rat_numerator(p) && is_rat_denominator(q)
}

/// A Float numeral triple: mantissa and exponent are Ints, the precision matches `[1-9][0-9]*`.
pub fn is_float(man: &str, exp: &str, prec: &str) -> bool {
    is_int(man) && is_int(exp) && is_positive(prec)
}

/// A non-negative decimal numeral `0|[1-9][0-9]*` within `u64`: a step budget, or a wall time.
pub fn parse_u64(s: &str) -> Option<u64> {
    if s == "0" || is_positive(s) {
        s.parse().ok()
    } else {
        None
    }
}
