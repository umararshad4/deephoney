//! DeepHoney research engine building blocks.
pub mod extract;
pub mod ghost;
pub mod ledger;
pub mod llm;
pub mod net;
pub mod report;
pub mod research;
pub use research::DeepResearch;
pub mod sources;
pub mod store;
mod tor;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Normal,
    Hidden,
    Dark,
    Adversarial,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Normal, Mode::Hidden, Mode::Dark, Mode::Adversarial];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "Normal",
            Mode::Hidden => "Hidden data",
            Mode::Dark => "Dark web",
            Mode::Adversarial => "Adversarial",
        }
    }

    pub fn strata(self) -> &'static [Stratum] {
        use Stratum::*;
        match self {
            Mode::Normal => &[Surface, Deep],
            Mode::Hidden | Mode::Adversarial => &[Surface, Deep, Buried],
            Mode::Dark => &[Surface, Deep, Buried, Dark],
        }
    }

    pub fn digs(self) -> bool {
        self.strata().contains(&Stratum::Buried)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stratum {
    #[default]
    Surface,
    Deep,
    Buried,
    Dark,
}

impl Stratum {
    pub fn label(self) -> &'static str {
        match self {
            Stratum::Surface => "surface",
            Stratum::Deep => "deep",
            Stratum::Buried => "buried",
            Stratum::Dark => "dark",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    Anthropic,
    /// Any OpenAI-compatible endpoint: Ollama, LM Studio, OpenRouter.
    OpenAi,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub llm_key: String,
    pub llm_base: String,
    pub strong_model: String,
    pub fast_model: String,
    pub brave_key: String,
    pub exa_key: String,
    pub tavily_key: String,
    pub searxng_url: String,
    pub github_token: String,
    pub courtlistener_token: String,
    /// Sent in the User-Agent; SEC EDGAR requires a contact.
    pub contact: String,
    pub tor_enabled: bool,
    pub max_pages: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            llm_key: String::new(),
            llm_base: "http://localhost:11434/v1".into(),
            strong_model: "claude-opus-5-5".into(),
            fast_model: "claude-sonnet-5-5".into(),
            brave_key: String::new(),
            exa_key: String::new(),
            tavily_key: String::new(),
            searxng_url: String::new(),
            github_token: String::new(),
            courtlistener_token: String::new(),
            contact: String::new(),
            tor_enabled: false,
            max_pages: 160,
        }
    }
}

impl Config {
    /// Headless config: ANTHROPIC_API_KEY, or OPENAI_BASE_URL (+ OPENAI_API_KEY, MODEL), plus search keys.
    pub fn from_env() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let mut c = Config::default();
        if !env("OPENAI_BASE_URL").is_empty() {
            c.provider = Provider::OpenAi;
            c.llm_base = env("OPENAI_BASE_URL");
            c.llm_key = env("OPENAI_API_KEY");
            c.strong_model = env("MODEL");
            c.fast_model = env("MODEL");
        } else {
            c.llm_key = env("ANTHROPIC_API_KEY");
        }
        c.brave_key = env("BRAVE_API_KEY");
        c.exa_key = env("EXA_API_KEY");
        c.tavily_key = env("TAVILY_API_KEY");
        c.searxng_url = env("SEARXNG_URL");
        c.github_token = env("GITHUB_TOKEN");
        c.courtlistener_token = env("COURTLISTENER_TOKEN");
        c.contact = env("DEEPHONEY_CONTACT");
        c.tor_enabled = env("DEEPHONEY_TOR") == "1";
        c
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub topic: String,
    pub mode: Mode,
    /// Rounds of the gap loop.
    pub depth: usize,
    /// Queries per lens in round one; halves each round.
    pub breadth: usize,
    #[serde(default)]
    pub plan: Option<ResearchPlan>,
}

impl Request {
    pub fn search_budget(&self) -> usize {
        let mut breadth = self.breadth.min(6);
        let mut total = 16;
        for _ in 0..self.depth.min(5) {
            total += 8 * 2 * breadth;
            breadth = (breadth / 2).max(1);
        }
        total + if self.mode == Mode::Adversarial { 8 } else { 0 }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResearchPlan {
    #[serde(default)]
    pub done: bool,
    pub lenses: Vec<Lens>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lens {
    pub name: String,
    pub question: String,
    pub sources: Vec<String>,
    pub queries: Vec<String>,
}

/// Progress stream for the UI / CLI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Plan(ResearchPlan),
    Round { round: usize, new_claims: usize, pages: usize, remaining: usize },
    Status(String),
    Query { lens: String, source: String, stratum: Stratum, query: String },
    Page { lens: String, stratum: Stratum, url: String, title: String },
    Claim { lens: String, text: String },
    Pivot { kind: String, value: String },
    Ghost { url: String, removed: usize },
    Stats { pages: usize, sources: usize, voices: usize, claims: usize },
    Warn(String),
}

pub type Tx = tokio::sync::mpsc::UnboundedSender<Event>;

/// Finished research, as shown in the app and saved to disk.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub topic: String,
    pub mode: Mode,
    pub markdown: String,
    pub ledger: ledger::Ledger,
    pub ghosts: Vec<ghost::Ghost>,
}

pub type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// Plug-in point for the research loop. The app streams `Event`s from `tx` and shows the returned `Report`.
/// Build it from the pieces in this crate: `sources::Source::search`, `net::Net::page`,
/// `extract::identifiers`, `ledger::Ledger`, `ghost`, and `llm::Llm`.
pub trait Researcher: Send + Sync + 'static {
    fn plan(&self, cfg: Config, req: Request) -> BoxFuture<anyhow::Result<ResearchPlan>>;
    fn run(&self, cfg: Config, req: Request, tx: Tx) -> BoxFuture<anyhow::Result<Report>>;
}
