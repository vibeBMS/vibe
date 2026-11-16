//! Requests as data. Whatever reads the network here is a [`Task`]: it says
//! which request it needs next ([`Next::Ask`]) and is fed the answer, until
//! it is done. A task does no I/O itself, so the same task runs natively
//! ([`run`] with an [`Http`] such as [`Client`]), in a web page (through a
//! CORS proxy, asynchronously) and in tests (canned answers).

/// A request a task needs answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Get(String),
    /// A POST of a JSON body (the MEGA API).
    Post {
        url: String,
        body: Vec<u8>,
    },
}

impl Request {
    pub fn url(&self) -> &str {
        match self {
            Request::Get(u) | Request::Post { url: u, .. } => u,
        }
    }
}

/// The body of a successful answer, or why there is none ("HTTP 404"...).
pub type Answer = Result<Vec<u8>, String>;

/// What a task needs next.
#[derive(Debug, PartialEq, Eq)]
pub enum Next<T> {
    Ask(Request),
    Done(T),
}

/// A computation that needs requests answered.
pub trait Task: Send {
    type Output;
    /// The first step.
    fn start(&mut self) -> Next<Self::Output>;
    /// The answer to the last [`Next::Ask`]: the next step.
    fn feed(&mut self, answer: Answer) -> Next<Self::Output>;
}

/// A task of any kind with this output.
pub type BoxTask<T> = Box<dyn Task<Output = T>>;

impl<T> Task for BoxTask<T> {
    type Output = T;
    fn start(&mut self) -> Next<T> {
        (**self).start()
    }
    fn feed(&mut self, answer: Answer) -> Next<T> {
        (**self).feed(answer)
    }
}

/// A task that is done at once.
pub struct Ready<T>(Option<T>);

impl<T: Send> Ready<T> {
    pub fn boxed(value: T) -> BoxTask<T>
    where
        T: 'static,
    {
        Box::new(Ready(Some(value)))
    }
}

impl<T: Send> Task for Ready<T> {
    type Output = T;
    fn start(&mut self) -> Next<T> {
        Next::Done(self.0.take().expect("a Ready task starts once"))
    }
    fn feed(&mut self, _: Answer) -> Next<T> {
        self.start()
    }
}

/// What answers requests.
pub trait Http {
    fn send(&self, request: &Request) -> Answer;
}

/// Run `task` to its end, asking `http`.
pub fn run<T: Task + ?Sized>(task: &mut T, http: &dyn Http) -> T::Output {
    let mut next = task.start();
    loop {
        match next {
            Next::Ask(r) => next = task.feed(http.send(&r)),
            Next::Done(out) => return out,
        }
    }
}

/// Canned answers by URL, for tests.
#[cfg(test)]
pub struct Canned(pub Vec<(String, Answer)>);

#[cfg(test)]
impl Http for Canned {
    fn send(&self, r: &Request) -> Answer {
        self.0
            .iter()
            .find(|(u, _)| u == r.url())
            .map(|(_, a)| a.clone())
            .unwrap_or_else(|| Err(format!("no answer for {}", r.url())))
    }
}

/// Blocking requests (desktop): the user agent of the [`Config`](crate::config::Config).
#[cfg(not(target_arch = "wasm32"))]
pub struct Client {
    pub user_agent: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl Client {
    pub fn new(config: &crate::config::Config) -> Client {
        Client {
            user_agent: config.user_agent.clone(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Http for Client {
    fn send(&self, r: &Request) -> Answer {
        let mut req = match r {
            Request::Get(u) => ehttp::Request::get(u),
            Request::Post { url, body } => {
                let mut q = ehttp::Request::post(url, body.clone());
                q.headers.insert("Content-Type", "application/json");
                q
            }
        };
        if !self.user_agent.is_empty() {
            req.headers.insert("User-Agent", &self.user_agent);
        }
        answer(ehttp::fetch_blocking(&req))
    }
}

/// A page or file in memory (a table, a list): a blocking GET with the
/// default settings.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(url: &str) -> Answer {
    Client::new(&crate::config::Config::default()).send(&Request::Get(url.into()))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn answer(r: Result<ehttp::Response, String>) -> Answer {
    match r {
        Ok(r) if r.ok => Ok(r.bytes),
        Ok(r) => Err(format!("HTTP {}", r.status)),
        Err(e) => Err(e),
    }
}

/// `s` percent-encoded for a URL's query.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `%XX` decoded (invalid UTF-8 replaced).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asks for two pages and gives their lengths.
    struct Two(Vec<usize>);
    impl Task for Two {
        type Output = Vec<usize>;
        fn start(&mut self) -> Next<Vec<usize>> {
            Next::Ask(Request::Get("a".into()))
        }
        fn feed(&mut self, a: Answer) -> Next<Vec<usize>> {
            self.0.push(a.map_or(0, |b| b.len()));
            if self.0.len() == 1 {
                Next::Ask(Request::Get("b".into()))
            } else {
                Next::Done(std::mem::take(&mut self.0))
            }
        }
    }

    #[test]
    fn runs() {
        let http = Canned(vec![
            ("a".into(), Ok(b"12".to_vec())),
            ("b".into(), Err("x".into())),
        ]);
        assert_eq!(run(&mut Two(Vec::new()), &http), [2, 0]);
        let mut boxed: BoxTask<Vec<usize>> = Box::new(Two(Vec::new()));
        assert_eq!(run(&mut boxed, &http), [2, 0]);
        assert_eq!(run(&mut Ready::boxed(7), &http), 7);
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_decode("%E6%9B%B2%2"), "曲%2");
    }
}
