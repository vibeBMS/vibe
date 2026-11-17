//! A request budget per service: a token bucket (`Rate::per_min` on
//! average, `Rate::burst` at once) and a bound on requests in flight. The
//! time is passed in.

use crate::provider::Rate;

/// One token, in the bucket's units: `per_min` units come per ms.
const TOKEN: u64 = 60_000;

#[derive(Clone, Debug)]
pub struct Limiter {
    rate: Rate,
    /// In units of `TOKEN`.
    level: u64,
    at_ms: i64,
    in_flight: u32,
    /// Nothing before this (a `Retry-After`).
    hold_ms: i64,
}

impl Limiter {
    /// A full bucket at `now_ms`.
    pub fn new(rate: Rate, now_ms: i64) -> Limiter {
        Limiter {
            rate,
            level: rate.burst as u64 * TOKEN,
            at_ms: now_ms,
            in_flight: 0,
            hold_ms: i64::MIN,
        }
    }

    fn refill(&mut self, now_ms: i64) {
        let ms = now_ms.saturating_sub(self.at_ms).max(0) as u64;
        let cap = self.rate.burst as u64 * TOKEN;
        self.level = (self.level + ms.saturating_mul(self.rate.per_min as u64)).min(cap);
        self.at_ms = self.at_ms.max(now_ms);
    }

    /// Start a request at `now_ms`, or `Err(ms)`: how long until one may
    /// start (0 while the requests in flight are at the bound: wait for
    /// one to [`finish`](Limiter::finish)).
    pub fn start(&mut self, now_ms: i64) -> Result<(), u64> {
        if now_ms < self.hold_ms {
            return Err((self.hold_ms - now_ms) as u64);
        }
        if self.in_flight >= self.rate.concurrent {
            return Err(0);
        }
        self.refill(now_ms);
        if self.level < TOKEN {
            let per_ms = self.rate.per_min.max(1) as u64;
            return Err((TOKEN - self.level).div_ceil(per_ms));
        }
        self.level -= TOKEN;
        self.in_flight += 1;
        Ok(())
    }

    /// A request started is answered (or failed).
    pub fn finish(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }

    /// Start nothing before `until_ms` (the server asked to wait).
    pub fn hold(&mut self, until_ms: i64) {
        self.hold_ms = self.hold_ms.max(until_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: Rate = Rate {
        per_min: 60,
        burst: 3,
        concurrent: 2,
    };

    #[test]
    fn a_burst_then_the_average() {
        let mut l = Limiter::new(RATE, 0);
        assert_eq!(l.start(0), Ok(()));
        l.finish();
        assert_eq!(l.start(0), Ok(()));
        l.finish();
        assert_eq!(l.start(0), Ok(()));
        l.finish();
        // empty: one a second
        assert_eq!(l.start(0), Err(1000));
        assert_eq!(l.start(999), Err(1));
        assert_eq!(l.start(1000), Ok(()));
        l.finish();
        // a long pause fills it to the burst only
        for _ in 0..3 {
            assert_eq!(l.start(600_000), Ok(()));
            l.finish();
        }
        assert!(l.start(600_000).is_err());
    }

    #[test]
    fn requests_in_flight_are_bounded() {
        let mut l = Limiter::new(RATE, 0);
        assert_eq!(l.start(0), Ok(()));
        assert_eq!(l.start(0), Ok(()));
        assert_eq!(l.start(0), Err(0));
        l.finish();
        assert_eq!(l.start(0), Ok(()));
    }

    #[test]
    fn a_hold_stops_everything_until_it_ends() {
        let mut l = Limiter::new(RATE, 0);
        l.hold(60_000);
        l.hold(10_000);
        assert_eq!(l.start(30_000), Err(30_000));
        assert_eq!(l.start(60_000), Ok(()));
    }

    #[test]
    fn a_minute_of_requests_stays_in_the_budget() {
        // a scroll asking every 10 ms for a minute
        let mut l = Limiter::new(RATE, 0);
        let mut sent = 0;
        for t in (0..60_000).step_by(10) {
            if l.start(t).is_ok() {
                sent += 1;
                l.finish();
            }
        }
        assert!(sent <= RATE.per_min + RATE.burst, "{sent}");
    }
}
